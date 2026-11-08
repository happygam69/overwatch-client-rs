//! The content surface: unlocks, cosmetics, loot boxes, achievements, seasons.
//!
//! These are the assets behind the *product* — what a player owns and how they
//! get it — as opposed to the assets behind gameplay. They are worth reading
//! for two reasons: they are the largest body of data in the install that
//! nothing here touched, and one of them, the skin theme, is the only route from
//! "this cosmetic exists" to "this is what it looks like" — the join the
//! renderer needs and does not have.
//!
//! ## Where the numbers come from
//!
//! Asset type ids come from the reference tooling's own switch list
//! (`TankTonka/Models/Common.cs`). Class and field hashes come from its
//! generated STU declarations (`TankLib/STU/Types/*.cs`), which are a
//! *published reconstruction* — a claim about structure, not a measurement of
//! this install. Every hash this module hardcodes is cited to the file and line
//! it was read from, and `docs/formats/content.md` records which of them were
//! then checked against real bytes.
//!
//! ## Most field names here are hashes, and that is expected
//!
//! The reference tooling could identify only 3,910 of the 23,204 fields its own
//! generator declares, and those cover 480 distinct field hashes. So a field
//! called `m_displayName` would be the exception, not the rule. Where the
//! reference tooling itself only has a hash, this module keeps the hash in the
//! name (`UNNAMED_F57B051E`) rather than inventing a word. A name this module
//! prints is therefore always a *reference-tooling* name, never one fitted here.
//!
//! ## Which subclass means which unlock
//!
//! An unlock asset holds one instance, and its *exact* class is what says what
//! kind of unlock it is. That mapping is [`unlock_kind_of`], transcribed from
//! the reference tooling's own `Unlock.GetUnlockType`
//! (`DataTool/DataModels/Unlock.cs:218`). It matches classes, not subtrees, so
//! the order it lists in is load-bearing: the souvenir class derives from the
//! emote class and has to be tested first. The tooling says the same thing in a
//! comment at `Unlock.cs:222`.
//!
//! ## The one thing that is *not* here: what a skin looks like
//!
//! [`SkinTheme`] gives the models, the portrait texture, the per-weapon-slot
//! skin assets and the team colour. It does **not** give the materials or the
//! body textures. Those are not in the skin theme asset at all: the reference
//! tooling builds them from the *resource graph's skin block*
//! (`DataTool/SaveLogic/Unlock/SkinTheme.cs:236`, `GetReplacements`), which maps
//! a base asset to the asset that replaces it for a given skin. `trg.rs` reads
//! the resource graph's package block and not its skin block, so that join is
//! still open. See `docs/formats/content.md` § "The skin join that is still
//! missing".
//!
//! ## Nothing here needs the class registry
//!
//! `classreg::ClassRegistry` is loaded by parsing the operator's OWLib checkout
//! at runtime and is **empty without it** (`keytable::Sources`, `OW_OWLIB`).
//! Everything in this module is therefore read the way `entity.rs` and
//! `settings.rs` read: from the hashes, straight off the file's own field bag.
//! The typed layer can name the fields it finds, and `tests/unlocks_real.rs`
//! cross-checks against it *when a checkout is present* — printing that the
//! cross-check was skipped when it is not, rather than passing silently.

use std::collections::BTreeMap;
use std::fmt;

use crate::{guid, GameData};
use ow_stu::Stu;

// ---------------------------------------------------------------------------
// Asset types
// ---------------------------------------------------------------------------

/// Type ids of the content surface. Read from `TankTonka/Models/Common.cs`, the
/// reference tooling's own enumeration, at the lines named on each.
pub mod types {
    /// `ProgressionUnlocks = 0x58` (`TankTonka/Models/Common.cs:37`).
    pub const PROGRESSION_UNLOCKS: u16 = 0x058;
    /// `Stat = 0x62` (`TankTonka/Models/Common.cs:40`).
    pub const STAT: u16 = 0x062;
    /// `Catalog = 0x63` (`TankTonka/Models/Common.cs:41`).
    pub const CATALOG: u16 = 0x063;
    /// `Achievement = 0x68` (`TankTonka/Models/Common.cs:43`).
    pub const ACHIEVEMENT: u16 = 0x068;
    /// `Unlock = 0xA5` (`TankTonka/Models/Common.cs:59`).
    pub const UNLOCK: u16 = 0x0A5;
    /// `SkinTheme = 0xA6` (`TankTonka/Models/Common.cs:60`).
    pub const SKIN_THEME: u16 = 0x0A6;
    /// `RankedSeason = 0xC8` (`TankTonka/Models/Common.cs:76`).
    pub const RANKED_SEASON: u16 = 0x0C8;
    /// `LootBox = 0xCF` (`TankTonka/Models/Common.cs:79`).
    pub const LOOT_BOX: u16 = 0x0CF;
    /// `VoiceConversation = 0xD0` (`TankTonka/Models/Common.cs:80`).
    pub const VOICE_CONVERSATION: u16 = 0x0D0;
    /// `CustomizableSkinTheme = 0x103` (`TankTonka/Models/Common.cs:82`).
    pub const CUSTOMIZABLE_SKIN_THEME: u16 = 0x103;

    /// `ModelLook = 0x1A` (`TankTonka/Models/Common.cs:17`). Not a content
    /// surface of its own, but the type a loot box's materials hang off, so it
    /// is named here rather than repeated as a bare literal.
    pub const MODEL_LOOK: u16 = 0x01A;

    /// Every content type, in the order [`super::survey_content`] walks them.
    pub const ALL: &[u16] = &[
        PROGRESSION_UNLOCKS,
        STAT,
        CATALOG,
        ACHIEVEMENT,
        UNLOCK,
        SKIN_THEME,
        RANKED_SEASON,
        LOOT_BOX,
        VOICE_CONVERSATION,
        CUSTOMIZABLE_SKIN_THEME,
    ];

    /// Human-readable name, for the CLI and for error messages.
    pub fn label(type_id: u16) -> &'static str {
        match type_id {
            PROGRESSION_UNLOCKS => "progression unlocks",
            STAT => "stat",
            CATALOG => "catalog",
            ACHIEVEMENT => "achievement",
            UNLOCK => "unlock",
            SKIN_THEME => "skin theme",
            RANKED_SEASON => "ranked season",
            LOOT_BOX => "loot box",
            VOICE_CONVERSATION => "voice conversation",
            CUSTOMIZABLE_SKIN_THEME => "customizable skin theme",
            MODEL_LOOK => "model look",
            _ => "content",
        }
    }

    /// The type a subcommand name selects: matched against [`label`] with
    /// spaces removed and case ignored, or against a literal `0x0A5`-style id.
    pub fn by_name(name: &str) -> Option<u16> {
        let squashed: String = name
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        ALL.iter().copied().find(|t| {
            let label: String = label(*t).chars().filter(|c| !c.is_whitespace()).collect();
            label == squashed || format!("{t:#05x}") == squashed
        })
    }
}

// ---------------------------------------------------------------------------
// Class hashes
// ---------------------------------------------------------------------------

/// The `STUUnlock` hierarchy, and the classes the other surfaces hang off.
///
/// The three intermediate classes are not decoration. `STUUnlock` is the base;
/// `STU_3021DDED` adds the display fields (name, description, rarity,
/// categories) every "traditional" unlock carries; `STU_895A2A7A` is an empty
/// further subclass that the non-cosmetic unlocks — currency, loot box, battle
/// pass — derive from instead. An unlock asset whose root is exactly
/// `STU_3021DDED` or `STU_895A2A7A` is a real shipped shape, and the reference
/// tooling's own type mapping calls it `Unknown`; so does [`unlock_kind_of`].
pub mod class {
    /// `STUUnlock` — `TankLib/STU/Types/STUUnlock.cs:7`.
    pub const UNLOCK: u32 = 0x6760_479E;
    /// `STU_3021DDED : STUUnlock` — `TankLib/STU/Types/Misc_9.cs:1739`.
    pub const UNLOCK_DISPLAY: u32 = 0x3021_DDED;
    /// `STU_895A2A7A : STU_3021DDED` — `TankLib/STU/Types/Misc_6.cs:5543`.
    pub const UNLOCK_PLAIN: u32 = 0x895A_2A7A;
    /// `STUUnlocks`, the holder for an unlock list —
    /// `TankLib/STU/Types/STUUnlocks.cs:6`.
    pub const UNLOCKS: u32 = 0x5ABF_8769;

    /// `STUUnlock_SkinTheme` — `STUUnlock_SkinTheme.cs:7`.
    pub const UNLOCK_SKIN_THEME: u32 = 0xE05E_4446;
    /// `STUUnlock_AvatarPortrait` — `STUUnlock_AvatarPortrait.cs:6`.
    pub const UNLOCK_AVATAR_PORTRAIT: u32 = 0x54BC_2188;
    /// `STU_A458D547`, which derives from the emote class and is a souvenir.
    /// Tested before [`UNLOCK_EMOTE`] for that reason — `Misc_1.cs:5879`.
    pub const SOUVENIR: u32 = 0xA458_D547;
    /// `STUUnlock_Emote` — `STUUnlock_Emote.cs:7`.
    pub const UNLOCK_EMOTE: u32 = 0x0B51_7D2E;
    /// `STUUnlock_Pose` — `STUUnlock_Pose.cs:6`.
    pub const UNLOCK_POSE: u32 = 0xC6A7_2877;
    /// `STUUnlock_VoiceLine` — `STUUnlock_VoiceLine.cs:6`.
    pub const UNLOCK_VOICE_LINE: u32 = 0x8A9D_1F89;
    /// `STUUnlock_SprayPaint` — `STUUnlock_SprayPaint.cs:6`.
    pub const UNLOCK_SPRAY_PAINT: u32 = 0x8451_5D93;
    /// `STUUnlock_Currency` — `STUUnlock_Currency.cs:6`.
    pub const UNLOCK_CURRENCY: u32 = 0x614B_C677;
    /// `STUUnlock_PortraitFrame` — `STUUnlock_PortraitFrame.cs:7`.
    pub const UNLOCK_PORTRAIT_FRAME: u32 = 0x6FE9_1269;
    /// `STUUnlock_Weapon` — `STUUnlock_Weapon.cs:7`.
    pub const UNLOCK_WEAPON: u32 = 0xC23F_89EB;
    /// `STUUnlock_POTGAnimation` — `STUUnlock_POTGAnimation.cs:7`.
    pub const UNLOCK_POTG_ANIMATION: u32 = 0x8E77_E8A1;
    /// `STUUnlock_CompetitiveCurrency` — `STUUnlock_CompetitiveCurrency.cs:6`.
    pub const UNLOCK_COMPETITIVE_CURRENCY: u32 = 0x136D_45F1;
    /// `STUUnlock_OWLToken` — `STUUnlock_OWLToken.cs:6`.
    pub const UNLOCK_OWL_TOKEN: u32 = 0xF2E7_4AF9;
    /// `STUUnlock_LootBox` — `STUUnlock_LootBox.cs:7`.
    pub const UNLOCK_LOOT_BOX: u32 = 0xC294_A43B;
    /// `STU_6A808718`, a weapon charm — `Misc_8.cs:4060`.
    pub const WEAPON_CHARM: u32 = 0x6A80_8718;
    /// `STU_C3C6FD9E`, a hero unlock — `Misc_4.cs:7630`.
    pub const HERO: u32 = 0xC3C6_FD9E;
    /// `STU_7A1A4764`, battle-pass XP — `Misc_4.cs:4204`.
    pub const BATTLE_PASS_XP: u32 = 0x7A1A_4764;
    /// `STU_DB1B05B5`, a name card — `Misc_7.cs:7420`.
    pub const NAME_CARD: u32 = 0xDB1B_05B5;
    /// `STU_514C0F6B`, a virtual-currency unlock — `Misc_9.cs:3026`.
    pub const VIRTUAL_CURRENCY: u32 = 0x514C_0F6B;
    /// `STU_52AB57E9`, a player title — `Misc_3.cs:2813`.
    pub const PLAYER_TITLE: u32 = 0x52AB_57E9;
    /// `STU_AD84E2AA`, a battle pass — `Misc_6.cs:6910`.
    pub const BATTLE_PASS: u32 = 0xAD84_E2AA;
    /// `STU_184D5944`, a season XP boost — `Misc_8.cs:1117`.
    pub const SEASON_XP_BOOST: u32 = 0x184D_5944;
    /// `STU_80C1169E`, a battle-pass tier skip — `Misc_6.cs:5204`.
    pub const BATTLE_PASS_TIER_SKIP: u32 = 0x80C1_169E;
    /// `STU_3F17D547`, a mythic-skin customization option — `Misc_5.cs:2456`.
    pub const SKIN_COMPONENT: u32 = 0x3F17_D547;
    /// `STU_A85D31BF`, a story mission — `Misc_1.cs:6066`.
    pub const STORY_MISSION: u32 = 0xA85D_31BF;
    /// `STU_2448F3AA`, a weapon skin — `Misc_6.cs:1189`.
    pub const WEAPON_SKIN: u32 = 0x2448_F3AA;
    /// `STU_FA317B7D`, a competitive signature — `Misc_7.cs:8192`.
    pub const COMPETITIVE_SIGNATURE: u32 = 0xFA31_7B7D;
    /// `STU_1EB22BDB`. The reference tooling maps it to `Unknown` on purpose
    /// rather than by falling through — `Misc_5.cs:1281`.
    pub const UNMAPPED_1EB22BDB: u32 = 0x1EB2_2BDB;

    /// `STUSkinBase` — `TankLib/STU/Types/STUSkinBase.cs:6`.
    pub const SKIN_BASE: u32 = 0xB7EE_A3BE;
    /// `STUSkinTheme : STUSkinBase` — `STUSkinTheme.cs:7`.
    pub const SKIN_THEME: u32 = 0x4227_0D59;
    /// `STU_21276722 : STUShippable`, the base every skin theme and skin
    /// sub-asset derives from — `Misc_4.cs:1208`.
    pub const SHIPPABLE_SKIN: u32 = 0x2127_6722;
    /// `STUShippable` — `STUShippable.cs:7`.
    pub const SHIPPABLE: u32 = 0x8FFA_F278;
    /// `STU_4BC3E632 : STUSkinBase`, the mythic-skin base holding the
    /// customization slots — `Misc_0.cs:2625`.
    pub const MYTHIC_SKIN_BASE: u32 = 0x4BC3_E632;
    /// `STU_EF85B312 : STU_4BC3E632`, a mythic skin — `Misc_4.cs:9063`.
    pub const MYTHIC_SKIN: u32 = 0xEF85_B312;
    /// `STU_47C34433`, one model a skin theme swaps in — `Misc_5.cs:2703`.
    pub const SKIN_MODEL: u32 = 0x47C3_4433;
    /// `STU_BE423F96`, one weapon slot a skin theme restyles — `Misc_4.cs:7392`.
    pub const SKIN_WEAPON: u32 = 0xBE42_3F96;
    /// `STU_882807BA`, a team-to-generated-theme mapping — `Misc_4.cs:4701`.
    pub const SKIN_TEAM_VARIANT: u32 = 0x8828_07BA;
    /// `STU_251C5714`, one mythic customization slot — `Misc_2.cs:1193`.
    pub const MYTHIC_SLOT: u32 = 0x251C_5714;

    /// `STULootBox` — `STULootBox.cs:7`.
    pub const LOOT_BOX: u32 = 0x56B6_D12E;
    /// `STULootBoxRules`, which carries the rarity weights — `STULootBoxRules.cs:7`.
    pub const LOOT_BOX_RULES: u32 = 0xAF3D_FC87;
    /// `STULootBoxRarityRules`, one rarity's weights — `STULootBoxRarityRules.cs:7`.
    pub const LOOT_BOX_RARITY_RULES: u32 = 0xEA8F_8295;
    /// `STULootBoxUnlocks`, a box's pool — `STULootBoxUnlocks.cs:7`.
    pub const LOOT_BOX_UNLOCKS: u32 = 0xF818_D652;
    /// `STULootBoxCurrencyUnlocks` — `STULootBoxCurrencyUnlocks.cs:7`.
    pub const LOOT_BOX_CURRENCY_UNLOCKS: u32 = 0x16F7_C0EA;
    /// `STULootBoxCurrencyRarityUnlock` — `STULootBoxCurrencyRarityUnlock.cs:7`.
    pub const LOOT_BOX_CURRENCY_RARITY_UNLOCK: u32 = 0x9FFC_F237;
    /// `STUGenericSettings_LootBoxes`, the settings asset that holds the rules
    /// array — `STUGenericSettings_LootBoxes.cs:6`.
    pub const GENERIC_SETTINGS_LOOT_BOXES: u32 = 0x9044_BCFE;

    /// `STUAchievement` — `STUAchievement.cs:6`.
    pub const ACHIEVEMENT: u32 = 0x0CC0_7049;
    /// `STURankedSeason` — `STURankedSeason.cs:6`.
    pub const RANKED_SEASON: u32 = 0xDDE9_37FD;
    /// `STURankedData`, the season's base — `STURankedData.cs:6`.
    pub const RANKED_DATA: u32 = 0x8159_A26D;
    /// `STUProgressionUnlocks` — `STUProgressionUnlocks.cs:6`.
    pub const PROGRESSION_UNLOCKS: u32 = 0x8CE1_D110;
    /// `STU_1757E817`, a level and the unlocks it grants — `Misc_5.cs:921`.
    pub const PROGRESSION_LEVEL: u32 = 0x1757_E817;
    /// `STUVoiceConversation` — `STUVoiceConversation.cs:6`.
    pub const VOICE_CONVERSATION: u32 = 0xA408_D74F;
    /// `STUVoiceConversationLine` — `STUVoiceConversationLine.cs:6`.
    pub const VOICE_CONVERSATION_LINE: u32 = 0x6BB3_C6F8;
    /// `STUStat` — `STUStat.cs:7`.
    pub const STAT: u32 = 0xC7BA_A017;
    /// `STUCatalog` — `STUCatalog.cs:6`.
    pub const CATALOG: u32 = 0x8561_8B21;
}

/// Field hashes, grouped by the class that declares them.
pub mod field {
    /// `STUUnlock` (`STUUnlock.cs:7`).
    ///
    /// Asset references are sixteen bytes wide with the GUID in the **first**
    /// eight, which is what [`ow_stu::Stu::read_asset_ref`] reads. The other
    /// eight are alignment. An *array element* puts the GUID at `+8` instead —
    /// see the note on [`unlocks::UNLOCKS`].
    pub mod unlock {
        /// `m_name`, the display-text asset.
        pub const NAME: u32 = 0xB48F_1D22;
        /// `m_resourceKey`, the store key the unlock is sold under.
        pub const RESOURCE_KEY: u32 = 0x38BF_B46C;
        /// `m_00B16A0B`, which the reference tooling calls `ProductId`
        /// (`Unlock.cs:133`) — a Battle.net catalogue identifier, not an amount.
        pub const PRODUCT_ID: u32 = 0x00B1_6A0B;
        /// `m_A7B393BF`, `Enum_5A7EBE59`: a *named* category, independent of
        /// the subclass. This is the one category field worth anything.
        pub const CATEGORY: u32 = 0xA7B3_93BF;
        /// `m_A649130B`, `Enum_B219A002`. No member names in the reference
        /// tooling, so this reader carries the raw number and nothing more.
        pub const UNNAMED_A649130B: u32 = 0xA649_130B;
    }

    /// `STU_3021DDED` (`Misc_9.cs:1739`).
    pub mod display {
        /// `m_53145FAF`, the "available in" text (`Unlock.cs:129`).
        pub const AVAILABLE_IN: u32 = 0x5314_5FAF;
        /// `m_3446F580`, the description text (`Unlock.cs:131`).
        pub const DESCRIPTION: u32 = 0x3446_F580;
        /// `m_BEE9BCDA`, the category list the gallery filters on
        /// (`Unlock.cs:144`). `STUIdentifier` references, and no identifier
        /// asset is shipped, so these can be counted but not named.
        pub const CATEGORIES: u32 = 0xBEE9_BCDA;
        /// `m_0B1BA7C1`, the team definition, for an esports unlock
        /// (`Unlock.cs:185`).
        pub const ESPORTS_TEAM: u32 = 0x0B1B_A7C1;
        /// `m_rarity`, `STUUnlockRarity`.
        pub const RARITY: u32 = 0xBB99_FCD3;
        /// `m_8EEF1251`. The reference tooling never reads it; it is named here
        /// because it is the only other small integer on the class.
        pub const UNNAMED_8EEF1251: u32 = 0x8EEF_1251;
    }

    /// `STUUnlock_SkinTheme` (`STUUnlock_SkinTheme.cs:10`).
    pub mod skin_unlock {
        /// `m_skinTheme`, the `STUSkinTheme` this unlock grants.
        pub const SKIN_THEME: u32 = 0x4C16_7404;
    }

    /// `STUUnlock_LootBox` (`STUUnlock_LootBox.cs:9`).
    pub mod loot_unlock {
        /// `m_lootBoxType`, `Enum_BABC4175`.
        pub const LOOT_BOX_TYPE: u32 = 0x7AB4_E3F8;
        /// `m_2F922165`, a `STUUnlockRarity` that replaces the inherited
        /// `m_rarity` for a box (`Unlock.cs:162`).
        pub const RARITY: u32 = 0x2F92_2165;
        /// `m_3CC2088A`, the hero, when the box is hero-specific.
        pub const HERO: u32 = 0x3CC2_088A;
    }

    /// `STUUnlock_PortraitFrame` (`STUUnlock_PortraitFrame.cs:10`).
    pub mod portrait_frame {
        /// `m_rankTexture`.
        pub const RANK_TEXTURE: u32 = 0xA4A6_6AB6;
        /// `m_rank`, `STUPortraitFrameRank`.
        pub const RANK: u32 = 0x8F73_6177;
        /// `m_level`, a `ushort`.
        pub const LEVEL: u32 = 0x2C01_908B;
        /// `m_stars`, a `ushort` immediately after `m_level`.
        pub const STARS: u32 = 0x78A2_AC5C;
    }

    /// `STUUnlock_Emote` (`STUUnlock_Emote.cs:10`).
    pub mod emote {
        /// `m_emote`, the `STUAnimAlias`.
        pub const EMOTE: u32 = 0xA0D4_ABDE;
        /// `m_28814AE5`, a `teVec3`.
        pub const UNNAMED_28814AE5: u32 = 0x2881_4AE5;
        /// `m_69150A1D`, a `teVec3`.
        pub const UNNAMED_69150A1D: u32 = 0x6915_0A1D;
    }

    /// `STUUnlock_Pose` (`STUUnlock_Pose.cs:9`).
    pub mod pose {
        /// `m_pose`.
        pub const POSE: u32 = 0x2E5F_BA99;
    }

    /// `STUUnlock_VoiceLine` (`STUUnlock_VoiceLine.cs:10`).
    pub mod voice_line {
        /// `m_F57B051E`. The reference tooling does not name it, so neither
        /// does this reader.
        pub const UNNAMED_F57B051E: u32 = 0xF57B_051E;
    }

    /// `STUUnlock_SprayPaint` (`STUUnlock_SprayPaint.cs:11`).
    pub mod spray {
        /// `m_E29A79B7`, unnamed in the reference tooling.
        pub const UNNAMED_E29A79B7: u32 = 0xE29A_79B7;
    }

    /// `STUUnlock_POTGAnimation` (`STUUnlock_POTGAnimation.cs:9`).
    pub mod potg {
        /// `m_animation`.
        pub const ANIMATION: u32 = 0x3F5B_86A4;
        /// `m_5530D275`, a `STUSkinTheme` — the highlight's skin.
        pub const SKIN_THEME: u32 = 0x5530_D275;
    }

    /// `STU_3F17D547`, a mythic customization option (`Misc_5.cs:2456`).
    pub mod skin_component {
        /// `m_E2A492C7`, the parent skin unlock (`Unlock.cs:194`).
        pub const PARENT_UNLOCK: u32 = 0xE2A4_92C7;
        /// `m_slot`, the slot's `STUIdentifier`.
        pub const SLOT: u32 = 0xF24E_4110;
        /// `m_4EC40A19`, the item within the slot.
        pub const ITEM: u32 = 0x4EC4_0A19;
    }

    /// `STU_6A808718`, a weapon charm (`Misc_8.cs:4060`).
    pub mod weapon_charm {
        /// `m_F1F888F9`, unnamed.
        pub const UNNAMED_F1F888F9: u32 = 0xF1F8_88F9;
        /// `m_06EE5924`, unnamed.
        pub const UNNAMED_06EE5924: u32 = 0x06EE_5924;
    }

    /// `STU_DB1B05B5`, a name card (`Misc_7.cs:7420`).
    pub mod name_card {
        /// `m_C5B31BBA`, unnamed.
        pub const UNNAMED_C5B31BBA: u32 = 0xC5B3_1BBA;
        /// `m_0CAFC9BA`, unnamed.
        pub const UNNAMED_0CAFC9BA: u32 = 0x0CAF_C9BA;
    }

    /// `STU_A85D31BF`, a story mission (`Misc_1.cs:6066`).
    pub mod story_mission {
        /// `m_216EA6DA`, unnamed.
        pub const UNNAMED_216EA6DA: u32 = 0x216E_A6DA;
    }

    /// `STU_2448F3AA`, a weapon skin (`Misc_6.cs:1189`).
    pub mod weapon_skin {
        /// `m_A9736011`, unnamed.
        pub const UNNAMED_A9736011: u32 = 0xA973_6011;
    }

    /// The classes whose only distinctive field is an amount. Each is read so
    /// [`crate::unlocks::Unlock::amount`] works uniformly; the label says which
    /// class the number came from, because six different fields all mean "how
    /// much".
    pub mod amount {
        /// `m_currency` on `STUUnlock_Currency` (`STUUnlock_Currency.cs:8`).
        pub const CURRENCY: u32 = 0x4CE5_6BC0;
        /// `m_760BF18E` on `STUUnlock_CompetitiveCurrency`
        /// (`STUUnlock_CompetitiveCurrency.cs:8`).
        pub const COMPETITIVE_CURRENCY: u32 = 0x760B_F18E;
        /// `m_63A026AF` on `STUUnlock_OWLToken` (`STUUnlock_OWLToken.cs:8`).
        pub const OWL_TOKEN: u32 = 0x63A0_26AF;
        /// `m_E0A45C1B` on `STU_7A1A4764`, battle-pass XP (`Misc_4.cs:4207`).
        pub const BATTLE_PASS_XP: u32 = 0xE0A4_5C1B;
        /// `m_D10618D1` on `STU_80C1169E`, battle-pass tiers (`Misc_6.cs:5207`).
        pub const BATTLE_PASS_TIER_SKIP: u32 = 0xD106_18D1;
        /// `m_AF872E86` on `STU_514C0F6B`, a virtual currency (`Misc_9.cs:3031`).
        pub const VIRTUAL_CURRENCY: u32 = 0xAF87_2E86;
        /// `m_level` on `STU_FA317B7D`, a signature's level (`Misc_7.cs:8216`).
        pub const SIGNATURE_LEVEL: u32 = 0x2C01_908B;
    }

    /// `STU_C3C6FD9E`, a hero unlock (`Misc_4.cs:7633`).
    pub mod hero_unlock {
        /// `m_hero`.
        pub const HERO: u32 = 0x37AB_13D3;
    }

    /// `STUSkinBase` (`STUSkinBase.cs:8`).
    pub mod skin_base {
        /// `m_5FC3164C`, the per-weapon-slot restyles, inline.
        pub const WEAPONS: u32 = 0x5FC3_164C;
        /// `m_BCC55571`, the models this skin swaps in, inline.
        pub const MODELS: u32 = 0xBCC5_5571;
        /// `m_50FDDF83`, a `STUTeamColor`.
        pub const UNNAMED_50FDDF83: u32 = 0x50FD_DF83;
    }

    /// `STU_47C34433`, a swapped-in model (`Misc_5.cs:2703`). `m_model` is the
    /// same field hash the entity's model component uses (`entity.rs`), which is
    /// what identifies it as a model rather than a hash collision.
    pub mod skin_model {
        /// `m_model`.
        pub const MODEL: u32 = 0x2D12_4BE2;
        /// `m_2F08CD5E`, an inline array of `STU_D9E3E761`. Its elements are not
        /// followed; only the count is carried.
        pub const UNNAMED_2F08CD5E: u32 = 0x2F08_CD5E;
    }

    /// `STU_BE423F96`, a weapon slot a skin restyles (`Misc_4.cs:7392`).
    pub mod skin_weapon {
        /// `m_BECFBDBA`, a `STU_649567C7` — itself an empty subclass of
        /// `STU_21276722`, so this is another skin asset rather than a model.
        pub const SKIN: u32 = 0xBECF_BDBA;
        /// `m_weaponType`, `STUWeaponType`.
        pub const WEAPON_TYPE: u32 = 0xCDEF_1203;
    }

    /// `STU_882807BA` (`Misc_4.cs:4701`).
    pub mod team_variant {
        /// `m_sourceTeam`.
        pub const SOURCE_TEAM: u32 = 0xF0A9_405A;
        /// `m_generatedSkinTheme`.
        pub const GENERATED_THEME: u32 = 0xC16A_0BCD;
    }

    /// `STUSkinTheme` (`STUSkinTheme.cs:10`).
    pub mod skin_theme {
        /// `m_internalName`, a `teString`. The only skin name stored as text
        /// rather than as a display-text asset reference.
        pub const INTERNAL_NAME: u32 = 0xA5C9_CA02;
        /// `m_ECCC4A5D`. The reference tooling saves this as the skin's
        /// portrait (`SaveLogic/Unlock/SkinTheme.cs:200`).
        pub const PORTRAIT: u32 = 0xECCC_4A5D;
        /// `m_71818BC5`, an unnamed array of references.
        pub const UNNAMED_71818BC5: u32 = 0x7181_8BC5;
        /// `m_E26643F0`, an unnamed single reference.
        pub const UNNAMED_E26643F0: u32 = 0xE266_43F0;
        /// `m_teamColor`.
        pub const TEAM_COLOR: u32 = 0xB8DC_6D46;
        /// `m_70D5F086`, the team-variant array, inline.
        pub const TEAM_VARIANTS: u32 = 0x70D5_F086;
        /// `m_02F889AE`, another `STUSkinTheme`.
        pub const UNNAMED_02F889AE: u32 = 0x02F8_89AE;
        /// `m_color`, a `teColorRGBA`.
        pub const COLOR: u32 = 0xC3E0_3C7F;
    }

    /// `STU_4BC3E632`, the mythic-skin base (`Misc_0.cs:2628`).
    pub mod mythic {
        /// `m_942A6CCA`, the customization slots, inline. `Unlock.cs:200` walks
        /// this list to name a skin component, which is what confirms it.
        pub const SLOTS: u32 = 0x942A_6CCA;
        /// `m_hero`.
        pub const HERO: u32 = 0x37AB_13D3;
    }

    /// `STU_251C5714`, one customization slot (`Misc_2.cs:1193`).
    pub mod mythic_slot {
        /// `m_id`, the slot's `STUIdentifier`.
        pub const ID: u32 = 0x11B4_7C68;
        /// `m_57CE9041`, the items the slot offers, inline.
        pub const ITEMS: u32 = 0x57CE_9041;
        /// `m_displayText`.
        pub const DISPLAY_TEXT: u32 = 0xAA76_FAD1;
        /// `m_texture`. The same hash `entity.rs` reads for a portrait.
        pub const TEXTURE: u32 = 0x3CD6_DC1E;
        /// `m_animation`.
        pub const ANIMATION: u32 = 0x3F5B_86A4;
    }

    /// `STULootBox` (`STULootBox.cs:9`).
    pub mod loot_box {
        /// `m_chestEntity`.
        pub const CHEST_ENTITY: u32 = 0xCBE2_DADD;
        /// `m_baseEntity`.
        pub const BASE_ENTITY: u32 = 0xB2F9_D222;
        /// `m_idleEffect`.
        pub const IDLE_EFFECT: u32 = 0x3970_E137;
        /// `m_modelLook`, the look the chest is drawn with.
        pub const MODEL_LOOK: u32 = 0x041C_E51F;
        /// `m_baseModelLook`, the look it reverts to when opened.
        pub const BASE_MODEL_LOOK: u32 = 0x9B18_0535;
        /// `m_name`.
        pub const NAME: u32 = 0xB48F_1D22;
        /// `m_celebration`.
        pub const CELEBRATION: u32 = 0xE02B_EE24;
        /// `m_shopCards`, inline.
        pub const SHOP_CARDS: u32 = 0xD755_86C0;
        /// `m_3DFAC8CA`, an unnamed reference array.
        pub const UNNAMED_3DFAC8CA: u32 = 0x3DFA_C8CA;
        /// `m_lootBoxType`, `Enum_BABC4175`.
        pub const LOOT_BOX_TYPE: u32 = 0x7AB4_E3F8;
        /// `m_hidePucks`, a byte.
        pub const HIDE_PUCKS: u32 = 0xFA2D_81E7;
    }

    /// `STUGenericSettings_LootBoxes` (`STUGenericSettings_LootBoxes.cs:8`).
    pub mod loot_settings {
        /// `m_lootboxes`, the `STULootBoxRules` array, inline.
        pub const RULES: u32 = 0x0BAD_9672;
    }

    /// `STULootBoxRules` (`STULootBoxRules.cs:9`).
    pub mod loot_rules {
        /// `m_rarityRules`, inline.
        pub const RARITY_RULES: u32 = 0xB7C0_634C;
        /// `m_currencyRarityRules`, inline.
        pub const CURRENCY_RARITY_RULES: u32 = 0x96D8_6FB8;
        /// `m_lootBoxType`, `Enum_BABC4175`.
        pub const LOOT_BOX_TYPE: u32 = 0x7AB4_E3F8;
        /// `m_24391F9B`, unnamed.
        pub const UNNAMED_24391F9B: u32 = 0x2439_1F9B;
        /// `m_53B8C818`, unnamed.
        pub const UNNAMED_53B8C818: u32 = 0x53B8_C818;
    }

    /// `STULootBoxRarityRules` (`STULootBoxRarityRules.cs:9`).
    pub mod rarity_rule {
        /// `m_rarity`, `STUUnlockRarity`.
        pub const RARITY: u32 = 0xBB99_FCD3;
        /// `m_A07D6F99`, an unnamed weight.
        pub const WEIGHT_A: u32 = 0xA07D_6F99;
        /// `m_CBE06580`, an unnamed weight.
        pub const WEIGHT_B: u32 = 0xCBE0_6580;
    }

    /// `STULootBoxUnlocks` (`STULootBoxUnlocks.cs:9`).
    pub mod loot_unlocks {
        /// `m_unlocks`, an inline `STUUnlocks`.
        pub const UNLOCKS: u32 = 0xDB80_3F2F;
        /// `m_lootBoxType`, `Enum_BABC4175`.
        pub const LOOT_BOX_TYPE: u32 = 0x7AB4_E3F8;
    }

    /// `STUAchievement` (`STUAchievement.cs:8`).
    pub mod achievement {
        /// `m_name`.
        pub const NAME: u32 = 0xB48F_1D22;
        /// `m_description`.
        pub const DESCRIPTION: u32 = 0xCA7E_6EDC;
        /// `m_290B2ADF`, unnamed.
        pub const UNNAMED_290B2ADF: u32 = 0x290B_2ADF;
        /// `m_245A3F6D`, a `STUTexture` — the achievement's icon.
        pub const ICON: u32 = 0x245A_3F6D;
        /// `m_544A6A4F`, a second `STUTexture` of unestablished purpose.
        pub const UNNAMED_544A6A4F: u32 = 0x544A_6A4F;
        /// `m_unlock`, the `STUUnlock` the achievement grants.
        pub const UNLOCK: u32 = 0xF508_7894;
        /// `m_4E291DCC`, a `teString`.
        pub const STRING: u32 = 0x4E29_1DCC;
        /// `m_value`, the progress target. The reference tooling has no better
        /// name for it and neither does this reader.
        pub const VALUE: u32 = 0x07DD_813E;
        /// `m_E1F5C73D`, unnamed.
        pub const UNNAMED_E1F5C73D: u32 = 0xE1F5_C73D;
        /// `m_628D48CC`, unnamed.
        pub const UNNAMED_628D48CC: u32 = 0x628D_48CC;
        /// `m_5351832E`, unnamed; the generated default is -1.
        pub const UNNAMED_5351832E: u32 = 0x5351_832E;
        /// The three config-var wrapper fields. Their element count says how many
        /// literals an achievement carries; counted here, decoded nowhere.
        pub const CONFIG_VAR_FIELDS: [u32; 3] = [0xCF16_B594, 0xFD43_B47A, 0x50AA_3D4C];
    }

    /// `STURankedData` (`STURankedData.cs:8`).
    pub mod ranked_data {
        /// `m_58066D8F`, an inline `STUUnlocks` — the season's rewards.
        pub const UNLOCKS: u32 = 0x5806_6D8F;
        /// `m_heroicUnlocks`, an inline `STUUnlocks`.
        pub const HEROIC_UNLOCKS: u32 = 0x3C18_94B2;
        /// `m_5BB8DFF3`, an inline array.
        pub const UNNAMED_5BB8DFF3: u32 = 0x5BB8_DFF3;
        /// `m_F82A8727`, a `teStructuredDataDateAndTime`, 8 bytes.
        pub const START_DATE: u32 = 0xF82A_8727;
        /// `m_275B6D5C`, a `teStructuredDataDateAndTime`, 8 bytes.
        pub const END_DATE: u32 = 0x275B_6D5C;
        /// `m_503BAA0A`, a float; the generated default is 168.
        pub const UNNAMED_503BAA0A: u32 = 0x503B_AAA0;
        /// `m_85059B03`, a `uint`; the generated default is 0x32.
        pub const UNNAMED_85059B03: u32 = 0x8505_9B03;
        /// `m_82AA08B4`, an int; the generated default is 10.
        pub const UNNAMED_82AA08B4: u32 = 0x82AA_08B4;
        /// `m_46B0B905`, an int; the generated default is 7.
        pub const UNNAMED_46B0B905: u32 = 0x46B0_B905;
        /// `m_842AF60F`, an int; the generated default is 25.
        pub const UNNAMED_842AF60F: u32 = 0x842A_F60F;
        /// `m_06122648`, an int; the generated default is 5.
        pub const UNNAMED_06122648: u32 = 0x0611_2648;
    }

    /// `STURankedSeason` (`STURankedSeason.cs:8`).
    pub mod ranked_season {
        /// `m_rankedSeason`, the season number.
        pub const SEASON: u32 = 0xC388_5423;
    }

    /// `STUProgressionUnlocks` (`STUProgressionUnlocks.cs:8`).
    pub mod progression {
        /// `m_9135A4B2`, an inline `STUUnlocks`.
        pub const UNLOCKS: u32 = 0x9135_A4B2;
        /// `m_otherUnlocks`, an inline `STUUnlocks`.
        pub const OTHER_UNLOCKS: u32 = 0xBF48_2AA3;
        /// `m_7846C401`, the per-level unlocks, inline.
        pub const LEVELS: u32 = 0x7846_C401;
        /// `m_lootBoxesUnlocks`, the per-box pools, inline.
        pub const LOOT_BOXES: u32 = 0x4734_94FF;
        /// `m_160B0466`, another progression-unlocks asset.
        pub const PARENT: u32 = 0x160B_0466;
    }

    /// `STU_1757E817` (`Misc_5.cs:921`).
    pub mod progression_level {
        /// `m_unlocks`, an inline `STUUnlocks`.
        pub const UNLOCKS: u32 = 0xDB80_3F2F;
        /// `m_level`.
        pub const LEVEL: u32 = 0x2C01_908B;
    }

    /// `STUVoiceConversation` (`STUVoiceConversation.cs:8`).
    pub mod conversation {
        /// `m_stimulus`, the `STUVoiceStimulus` this answers.
        pub const STIMULUS: u32 = 0x401F_5484;
        /// `m_265A80ED`, an unnamed `ushort[]` in the dynamic block.
        pub const UNNAMED_265A80ED: u32 = 0x265A_80ED;
        /// `m_90D76F17`, the lines, inline.
        pub const LINES: u32 = 0x90D7_6F17;
        /// `m_weight`; the generated default is 1.0f.
        pub const WEIGHT: u32 = 0x9CDD_C24D;
    }

    /// `STUVoiceConversationLine` (`STUVoiceConversationLine.cs:8`).
    pub mod conversation_line {
        /// `m_E295B99C`, the `STUVoiceLine`.
        pub const VOICE_LINE: u32 = 0xE295_B99C;
        /// `m_B4D405A1`, a `uint`. The reference tooling does not read it.
        pub const UNNAMED_B4D405A1: u32 = 0xB4D4_05A1;
    }

    /// `STUStat` (`STUStat.cs:9`).
    pub mod stat {
        /// `m_name`.
        pub const NAME: u32 = 0xB48F_1D22;
        /// `m_9F574E87`, unnamed.
        pub const UNNAMED_9F574E87: u32 = 0x9F57_4E87;
        /// `m_heroes`, a `teStructuredDataAssetRef<STUHero>[]`.
        pub const HEROES: u32 = 0x056D_3E39;
        /// `m_maps`, a `teStructuredDataAssetRef<STUMap>[]`.
        pub const MAPS: u32 = 0x6E01_378E;
        /// `m_teams`, a `TeamIndex[]`.
        pub const TEAMS: u32 = 0xD440_A0F7;
        /// `m_displayType`.
        pub const DISPLAY_TYPE: u32 = 0xBEA8_7015;
        /// `m_statCategory`.
        pub const CATEGORY: u32 = 0xF91D_6AD9;
        /// `m_type`, `STUStatType`.
        pub const TYPE: u32 = 0x0619_C597;
        /// `m_internalOnly`, a byte.
        pub const INTERNAL_ONLY: u32 = 0xC021_1FD4;
    }

    /// `STUCatalog` (`STUCatalog.cs:8`).
    pub mod catalog {
        /// `m_assets`, a bare `ulong[]`. **Not** a
        /// `teStructuredDataAssetRef[]`, so there is no per-element field hash to
        /// obfuscate with, and the values are read exactly as stored.
        pub const ASSETS: u32 = 0x3D83_C363;
        /// `m_B7FC20A3`, a bare `long[]`.
        pub const UNNAMED_B7FC20A3: u32 = 0xB7FC_20A3;
    }

    /// `STUUnlocks` (`STUUnlocks.cs:8`).
    pub mod unlocks {
        /// `m_unlocks`, a `teStructuredDataAssetRef<STUUnlock>[]` in the
        /// dynamic block.
        ///
        /// Note the element layout: an array element puts its GUID at **`+8`**,
        /// sixteen bytes apart, where a scalar asset reference puts it at `+0`.
        /// That is the typed layer's `read_element`
        /// (`ow_stu/src/types.rs:1030`), and `read_ref_array` follows it
        /// exactly rather than re-deriving it.
        pub const UNLOCKS: u32 = 0xDB80_3F2F;
    }
}

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// `STUUnlockRarity` (`TankLib/STU/Types/Enums/STUUnlockRarity.cs:6`, enum hash
/// `0x4CAFD36F`).
///
/// `Unnamed7` exists because the generated enum declares a member the reference
/// tooling could not name (`0xEF68A54A`); `Unknown` because a value outside the
/// set is carried rather than clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Rare,
    Epic,
    Legendary,
    Evolution,
    Mythic,
    Ultra,
    /// Value 7. The generated member is `xEF68A54A`: unnamed in the reference.
    Unnamed7,
    /// Value -1, which the generated enum does name `None`.
    None,
    /// A value outside the generated set.
    Unknown(i32),
}

impl Rarity {
    /// Read a `STUUnlockRarity` from its stored integer.
    pub fn from_raw(raw: i32) -> Rarity {
        match raw {
            0 => Rarity::Common,
            1 => Rarity::Rare,
            2 => Rarity::Epic,
            3 => Rarity::Legendary,
            4 => Rarity::Evolution,
            5 => Rarity::Mythic,
            6 => Rarity::Ultra,
            7 => Rarity::Unnamed7,
            -1 => Rarity::None,
            other => Rarity::Unknown(other),
        }
    }
}

impl fmt::Display for Rarity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Rarity::Common => "Common",
            Rarity::Rare => "Rare",
            Rarity::Epic => "Epic",
            Rarity::Legendary => "Legendary",
            Rarity::Evolution => "Evolution",
            Rarity::Mythic => "Mythic",
            Rarity::Ultra => "Ultra",
            Rarity::Unnamed7 => "Unnamed7",
            Rarity::None => "None",
            Rarity::Unknown(v) => return write!(f, "Unknown({v})"),
        })
    }
}

/// `Enum_BABC4175`, the loot-box theme (`TankLib/STU/Types/Enums/Misc.cs:4869`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootBoxKind {
    Base,
    SummerGames,
    Halloween,
    Winter,
    LunarNewYear,
    Archives,
    Anniversary,
    Golden,
    Internal,
    LegendaryAnniversary,
    Hammond,
    /// 0xB — unnamed in the generated enum.
    Unnamed0B,
    Legendary,
    /// 0xD — unnamed in the generated enum.
    Unnamed0D,
    Unknown(i32),
}

impl LootBoxKind {
    pub fn from_raw(raw: i32) -> LootBoxKind {
        match raw {
            0x0 => LootBoxKind::Base,
            0x1 => LootBoxKind::SummerGames,
            0x2 => LootBoxKind::Halloween,
            0x3 => LootBoxKind::Winter,
            0x4 => LootBoxKind::LunarNewYear,
            0x5 => LootBoxKind::Archives,
            0x6 => LootBoxKind::Anniversary,
            0x7 => LootBoxKind::Golden,
            0x8 => LootBoxKind::Internal,
            0x9 => LootBoxKind::LegendaryAnniversary,
            0xA => LootBoxKind::Hammond,
            0xB => LootBoxKind::Unnamed0B,
            0xC => LootBoxKind::Legendary,
            0xD => LootBoxKind::Unnamed0D,
            other => LootBoxKind::Unknown(other),
        }
    }

    /// The stored integer, whatever the value.
    pub fn raw(self) -> i32 {
        match self {
            LootBoxKind::Base => 0x0,
            LootBoxKind::SummerGames => 0x1,
            LootBoxKind::Halloween => 0x2,
            LootBoxKind::Winter => 0x3,
            LootBoxKind::LunarNewYear => 0x4,
            LootBoxKind::Archives => 0x5,
            LootBoxKind::Anniversary => 0x6,
            LootBoxKind::Golden => 0x7,
            LootBoxKind::Internal => 0x8,
            LootBoxKind::LegendaryAnniversary => 0x9,
            LootBoxKind::Hammond => 0xA,
            LootBoxKind::Unnamed0B => 0xB,
            LootBoxKind::Legendary => 0xC,
            LootBoxKind::Unnamed0D => 0xD,
            LootBoxKind::Unknown(v) => v,
        }
    }
}

impl fmt::Display for LootBoxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LootBoxKind::Base => "base",
            LootBoxKind::SummerGames => "summer games",
            LootBoxKind::Halloween => "halloween",
            LootBoxKind::Winter => "winter",
            LootBoxKind::LunarNewYear => "lunar new year",
            LootBoxKind::Archives => "archives",
            LootBoxKind::Anniversary => "anniversary",
            LootBoxKind::Golden => "golden",
            LootBoxKind::Internal => "internal",
            LootBoxKind::LegendaryAnniversary => "legendary anniversary",
            LootBoxKind::Hammond => "hammond",
            LootBoxKind::Unnamed0B => "unnamed(0x0B)",
            LootBoxKind::Legendary => "legendary",
            LootBoxKind::Unnamed0D => "unnamed(0x0D)",
            LootBoxKind::Unknown(_) => return write!(f, "unknown({})", self.raw()),
        })
    }
}

/// `Enum_5A7EBE59`, the category stored on `STUUnlock.m_A7B393BF`
/// (`TankLib/STU/Types/Enums/Misc.cs:2372`).
///
/// This is the useful one: it names the category of an unlock *without*
/// depending on which subclass it is, so an emote and a skin are comparable
/// without knowing either one's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Skin,
    VictoryPose,
    Emote,
    /// 3 — unnamed in the generated enum.
    Unnamed3,
    /// 4 — unnamed.
    Unnamed4,
    Souvenir,
    HighlightIntro,
    Spray,
    VoiceLine,
    WeaponSkin,
    WeaponCharm,
    Portrait,
    NameCard,
    PlayerTitle,
    /// 0xE — unnamed.
    Unnamed0E,
    Currency,
    CompetitiveCurrency,
    OverwatchLeagueCurrency,
    /// 0x12 — unnamed.
    Unnamed12,
    Lootbox,
    OverwatchCoins,
    BattlePass,
    /// 0x16 — unnamed.
    Unnamed16,
    BattlePassXp,
    Hero,
    SeasonXpBoost,
    Unnamed1A,
    Unnamed1B,
    Unnamed1C,
    Unnamed1D,
    Unnamed1E,
    Unnamed1F,
    Unnamed20,
    Unnamed21,
    Unknown(i32),
}

impl Category {
    pub fn from_raw(raw: i32) -> Category {
        match raw {
            0x00 => Category::Skin,
            0x01 => Category::VictoryPose,
            0x02 => Category::Emote,
            0x03 => Category::Unnamed3,
            0x04 => Category::Unnamed4,
            0x05 => Category::Souvenir,
            0x06 => Category::HighlightIntro,
            0x07 => Category::Spray,
            0x08 => Category::VoiceLine,
            0x09 => Category::WeaponSkin,
            0x0A => Category::WeaponCharm,
            0x0B => Category::Portrait,
            0x0C => Category::NameCard,
            0x0D => Category::PlayerTitle,
            0x0E => Category::Unnamed0E,
            0x0F => Category::Currency,
            0x10 => Category::CompetitiveCurrency,
            0x11 => Category::OverwatchLeagueCurrency,
            0x12 => Category::Unnamed12,
            0x13 => Category::Lootbox,
            0x14 => Category::OverwatchCoins,
            0x15 => Category::BattlePass,
            0x16 => Category::Unnamed16,
            0x17 => Category::BattlePassXp,
            0x18 => Category::Hero,
            0x19 => Category::SeasonXpBoost,
            0x1A => Category::Unnamed1A,
            0x1B => Category::Unnamed1B,
            0x1C => Category::Unnamed1C,
            0x1D => Category::Unnamed1D,
            0x1E => Category::Unnamed1E,
            0x1F => Category::Unnamed1F,
            0x20 => Category::Unnamed20,
            0x21 => Category::Unnamed21,
            other => Category::Unknown(other),
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Category::Skin => "skin",
            Category::VictoryPose => "victory pose",
            Category::Emote => "emote",
            Category::Unnamed3 => "unnamed(0x03)",
            Category::Unnamed4 => "unnamed(0x04)",
            Category::Souvenir => "souvenir",
            Category::HighlightIntro => "highlight intro",
            Category::Spray => "spray",
            Category::VoiceLine => "voice line",
            Category::WeaponSkin => "weapon skin",
            Category::WeaponCharm => "weapon charm",
            Category::Portrait => "portrait",
            Category::NameCard => "name card",
            Category::PlayerTitle => "player title",
            Category::Unnamed0E => "unnamed(0x0E)",
            Category::Currency => "currency",
            Category::CompetitiveCurrency => "competitive currency",
            Category::OverwatchLeagueCurrency => "OWL currency",
            Category::Unnamed12 => "unnamed(0x12)",
            Category::Lootbox => "loot box",
            Category::OverwatchCoins => "overwatch coins",
            Category::BattlePass => "battle pass",
            Category::Unnamed16 => "unnamed(0x16)",
            Category::BattlePassXp => "battle pass XP",
            Category::Hero => "hero",
            Category::SeasonXpBoost => "season XP boost",
            Category::Unnamed1A => "unnamed(0x1A)",
            Category::Unnamed1B => "unnamed(0x1B)",
            Category::Unnamed1C => "unnamed(0x1C)",
            Category::Unnamed1D => "unnamed(0x1D)",
            Category::Unnamed1E => "unnamed(0x1E)",
            Category::Unnamed1F => "unnamed(0x1F)",
            Category::Unnamed20 => "unnamed(0x20)",
            Category::Unnamed21 => "unnamed(0x21)",
            Category::Unknown(_) => "unknown",
        })
    }
}

/// `STUPortraitFrameRank` (`TankLib/STU/Types/Enums/STUPortraitFrameRank.cs:6`,
/// enum hash `0x161443EA`). The zero member is unnamed in the generated enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortraitFrameRank {
    Unnamed0,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Unknown(i32),
}

impl PortraitFrameRank {
    pub fn from_raw(raw: i32) -> PortraitFrameRank {
        match raw {
            0 => PortraitFrameRank::Unnamed0,
            1 => PortraitFrameRank::Bronze,
            2 => PortraitFrameRank::Silver,
            3 => PortraitFrameRank::Gold,
            4 => PortraitFrameRank::Platinum,
            5 => PortraitFrameRank::Diamond,
            other => PortraitFrameRank::Unknown(other),
        }
    }
}

impl fmt::Display for PortraitFrameRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PortraitFrameRank::Unnamed0 => "unnamed(0)",
            PortraitFrameRank::Bronze => "bronze",
            PortraitFrameRank::Silver => "silver",
            PortraitFrameRank::Gold => "gold",
            PortraitFrameRank::Platinum => "platinum",
            PortraitFrameRank::Diamond => "diamond",
            PortraitFrameRank::Unknown(v) => return write!(f, "unknown({v})"),
        })
    }
}

/// `STUWeaponType` (`TankLib/STU/Types/Enums/STUWeaponType.cs:6`, enum hash
/// `0x4E56D01D`).
///
/// It names a *skin variation*, not a weapon slot: the values are the
/// golden/jade/galactic weapon tiers. `SaveLogic/Unlock/SkinTheme.cs:185` uses
/// the same field to name a preview model, so both readings exist in the
/// reference tooling; this reader reports the field and does not choose between
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponVariation {
    Default,
    Golden,
    Jade,
    Galactic,
    CrimsonWolf,
    /// 5 — unnamed in the generated enum.
    Unnamed5,
    Unknown(i32),
}

impl WeaponVariation {
    pub fn from_raw(raw: i32) -> WeaponVariation {
        match raw {
            0 => WeaponVariation::Default,
            1 => WeaponVariation::Golden,
            2 => WeaponVariation::Jade,
            3 => WeaponVariation::Galactic,
            4 => WeaponVariation::CrimsonWolf,
            5 => WeaponVariation::Unnamed5,
            other => WeaponVariation::Unknown(other),
        }
    }
}

impl fmt::Display for WeaponVariation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WeaponVariation::Default => "default",
            WeaponVariation::Golden => "golden",
            WeaponVariation::Jade => "jade",
            WeaponVariation::Galactic => "galactic",
            WeaponVariation::CrimsonWolf => "crimson wolf",
            WeaponVariation::Unnamed5 => "unnamed(5)",
            WeaponVariation::Unknown(v) => return write!(f, "unknown({v})"),
        })
    }
}

// ---------------------------------------------------------------------------
// Unlocks
// ---------------------------------------------------------------------------

/// What kind of unlock a root class is, from the reference tooling's own mapping
/// (`DataTool/DataModels/Unlock.cs:218`).
///
/// The mapping is over *exact* classes. A subclass of a listed class that the
/// tooling does not list is [`UnlockKind::Unknown`], even though it inherits
/// everything the listed class declares. That is what the tooling does, and
/// following it beats second-guessing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockKind {
    Skin,
    Icon,
    Spray,
    Emote,
    VictoryPose,
    HighlightIntro,
    VoiceLine,
    WeaponVariant,
    Lootbox,
    PortraitFrame,
    Currency,
    CompetitiveCurrency,
    OwlToken,
    VirtualCurrency,
    SeasonXpBoost,
    WeaponCharm,
    PlayerTitle,
    BattlePass,
    BattlePassXp,
    Souvenir,
    NameCard,
    Hero,
    SkinComponent,
    BattlePassTierSkip,
    StoryMission,
    WeaponSkin,
    CompetitiveSignature,
    /// A class the tooling maps to `Unknown` on purpose
    /// (`STU_1EB22BDB`, `Unlock.cs:247`).
    Unmapped,
    /// Anything else, including `STUUnlock` itself and the two intermediate
    /// classes, which the tooling's switch does not list.
    Unknown,
}

impl UnlockKind {
    /// Whether the reference tooling treats this kind as an ordinary cosmetic or
    /// reward, as opposed to a currency, a box or a pass.
    ///
    /// Transcribed from `Unlock.cs:135`. That list is what decides whether
    /// "available in" and the category list are shown for a record.
    pub fn is_traditional(self) -> bool {
        matches!(
            self,
            UnlockKind::Icon
                | UnlockKind::Spray
                | UnlockKind::Skin
                | UnlockKind::HighlightIntro
                | UnlockKind::VictoryPose
                | UnlockKind::VoiceLine
                | UnlockKind::Emote
                | UnlockKind::Souvenir
                | UnlockKind::NameCard
                | UnlockKind::PlayerTitle
                | UnlockKind::WeaponCharm
                | UnlockKind::WeaponVariant
                | UnlockKind::WeaponSkin
        )
    }
}

impl fmt::Display for UnlockKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnlockKind::Skin => "Skin",
            UnlockKind::Icon => "Icon",
            UnlockKind::Spray => "Spray",
            UnlockKind::Emote => "Emote",
            UnlockKind::VictoryPose => "VictoryPose",
            UnlockKind::HighlightIntro => "HighlightIntro",
            UnlockKind::VoiceLine => "VoiceLine",
            UnlockKind::WeaponVariant => "WeaponVariant",
            UnlockKind::Lootbox => "Lootbox",
            UnlockKind::PortraitFrame => "PortraitFrame",
            UnlockKind::Currency => "Currency",
            UnlockKind::CompetitiveCurrency => "CompetitiveCurrency",
            UnlockKind::OwlToken => "OWLToken",
            UnlockKind::VirtualCurrency => "VirtualCurrency",
            UnlockKind::SeasonXpBoost => "SeasonXPBoost",
            UnlockKind::WeaponCharm => "WeaponCharm",
            UnlockKind::PlayerTitle => "PlayerTitle",
            UnlockKind::BattlePass => "BattlePass",
            UnlockKind::BattlePassXp => "BattlePassXP",
            UnlockKind::Souvenir => "Souvenir",
            UnlockKind::NameCard => "NameCard",
            UnlockKind::Hero => "Hero",
            UnlockKind::SkinComponent => "SkinComponent",
            UnlockKind::BattlePassTierSkip => "BattlePassTierSkip",
            UnlockKind::StoryMission => "StoryMission",
            UnlockKind::WeaponSkin => "WeaponSkin",
            UnlockKind::CompetitiveSignature => "CompetitiveSignature",
            UnlockKind::Unmapped => "Unmapped",
            UnlockKind::Unknown => "Unknown",
        })
    }
}

/// Which unlock type a root class is.
///
/// The order is the reference tooling's, and it matters once: `STU_A458D547` (a
/// souvenir) derives from the emote class, so testing the emote class first
/// would call a souvenir an emote. The tooling notes the same thing at
/// `Unlock.cs:222`.
pub fn unlock_kind_of(class_hash: u32) -> UnlockKind {
    match class_hash {
        class::UNLOCK_SKIN_THEME => UnlockKind::Skin,
        class::UNLOCK_AVATAR_PORTRAIT => UnlockKind::Icon,
        class::SOUVENIR => UnlockKind::Souvenir,
        class::UNLOCK_EMOTE => UnlockKind::Emote,
        class::UNLOCK_POSE => UnlockKind::VictoryPose,
        class::UNLOCK_VOICE_LINE => UnlockKind::VoiceLine,
        class::UNLOCK_SPRAY_PAINT => UnlockKind::Spray,
        class::UNLOCK_CURRENCY => UnlockKind::Currency,
        class::UNLOCK_PORTRAIT_FRAME => UnlockKind::PortraitFrame,
        class::UNLOCK_WEAPON => UnlockKind::WeaponVariant,
        class::UNLOCK_POTG_ANIMATION => UnlockKind::HighlightIntro,
        class::UNLOCK_COMPETITIVE_CURRENCY => UnlockKind::CompetitiveCurrency,
        class::UNLOCK_OWL_TOKEN => UnlockKind::OwlToken,
        class::UNLOCK_LOOT_BOX => UnlockKind::Lootbox,
        class::WEAPON_CHARM => UnlockKind::WeaponCharm,
        class::HERO => UnlockKind::Hero,
        class::BATTLE_PASS_XP => UnlockKind::BattlePassXp,
        class::NAME_CARD => UnlockKind::NameCard,
        class::VIRTUAL_CURRENCY => UnlockKind::VirtualCurrency,
        class::PLAYER_TITLE => UnlockKind::PlayerTitle,
        class::BATTLE_PASS => UnlockKind::BattlePass,
        class::SEASON_XP_BOOST => UnlockKind::SeasonXpBoost,
        class::BATTLE_PASS_TIER_SKIP => UnlockKind::BattlePassTierSkip,
        class::SKIN_COMPONENT => UnlockKind::SkinComponent,
        class::STORY_MISSION => UnlockKind::StoryMission,
        class::WEAPON_SKIN => UnlockKind::WeaponSkin,
        class::COMPETITIVE_SIGNATURE => UnlockKind::CompetitiveSignature,
        class::UNMAPPED_1EB22BDB => UnlockKind::Unmapped,
        _ => UnlockKind::Unknown,
    }
}

/// One asset reference an unlock carries, kept with the field that carried it.
///
/// The field hash is load-bearing rather than incidental: most fields on these
/// classes are ones the reference tooling could not name, so the hash is often
/// the only label that exists. Where it did name one, `label` carries *that*
/// name — never one chosen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub field: u32,
    pub label: Option<&'static str>,
    pub target: u64,
}

impl fmt::Display for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.label {
            Some(name) => write!(f, "{name}=0x{:016x}", self.target)?,
            None => write!(f, "0x{:016x}", self.target)?,
        }
        write!(
            f,
            " (type 0x{:03X}, field 0x{:08X})",
            guid::type_of(self.target),
            self.field
        )
    }
}

/// One unlock, as much as is currently understood.
#[derive(Debug, Clone, PartialEq)]
pub struct Unlock {
    /// GUID of the unlock asset this came from.
    pub asset: u64,
    /// The root instance's exact class.
    pub root_class: u32,
    pub kind: UnlockKind,
    /// `m_name`: a display-text *asset*, not a string. Turning it into text is
    /// the text layer's job.
    pub name: Option<u64>,
    pub description: Option<u64>,
    /// `m_53145FAF`, the "available in" text, e.g. the shop.
    pub available_in: Option<u64>,
    /// `m_rarity`.
    pub rarity: Option<Rarity>,
    /// `m_A7B393BF`, the named category, independent of `kind` and present on
    /// nearly every unlock.
    pub category: Option<Category>,
    /// `m_BEE9BCDA`, the gallery-filter category list. `STUIdentifier`
    /// references, and no identifier asset is shipped, so these can be counted
    /// but not named.
    pub categories: Vec<u64>,
    /// `m_00B16A0B`, the Battle.net product id. **Not a price** — see
    /// [`Unlock::price`].
    pub product_id: Option<i64>,
    /// `m_resourceKey`, the store key.
    pub resource_key: Option<u64>,
    /// `m_0B1BA7C1`, the team definition, for an esports unlock.
    pub esports_team: Option<u64>,
    /// `m_lootBoxType`, on a loot-box unlock.
    pub loot_box_kind: Option<LootBoxKind>,
    /// `m_rank`, on a portrait frame.
    pub portrait_rank: Option<PortraitFrameRank>,
    /// `m_level`, on a portrait frame or a competitive signature.
    pub level: Option<i32>,
    /// `m_stars`, on a portrait frame.
    pub stars: Option<i32>,
    /// The subclass's own references, labelled.
    pub grants: Vec<Grant>,
    /// The subclass's own integers, labelled.
    pub values: Vec<(&'static str, i32)>,
}

impl Unlock {
    /// The amount this unlock grants, for the classes that carry one.
    ///
    /// Six different fields all mean "how much" ([`field::amount`]), and which
    /// one applies depends on the subclass, so this folds them into one number
    /// and [`Unlock::amount_field`] says which field it came from. `m_rank` and
    /// `m_level` are deliberately excluded: they are typed into their own slots
    /// and are not amounts of anything.
    pub fn amount(&self) -> Option<i32> {
        self.amount_field().and_then(|label| {
            self.values
                .iter()
                .find(|(name, _)| *name == label)
                .map(|(_, v)| *v)
        })
    }

    /// Which field [`Unlock::amount`] read, so a caller that cares about *which
    /// kind* of amount it is can tell.
    pub fn amount_field(&self) -> Option<&'static str> {
        self.values
            .iter()
            .map(|(label, _)| *label)
            .find(|label| !matches!(*label, "m_rank" | "m_level" | "m_stars" | "m_weaponType"))
    }

    /// The `STUSkinTheme` this unlock grants, for a skin.
    pub fn skin_theme(&self) -> Option<u64> {
        self.grant(field::skin_unlock::SKIN_THEME)
    }

    /// The hero this unlock grants, for a hero unlock or a hero-specific box.
    pub fn hero(&self) -> Option<u64> {
        self.grant(field::hero_unlock::HERO)
            .or_else(|| self.grant(field::loot_unlock::HERO))
    }

    /// A grant by field hash.
    pub fn grant(&self, field_hash: u32) -> Option<u64> {
        self.grants
            .iter()
            .find(|g| g.field == field_hash)
            .map(|g| g.target)
    }

    /// What this unlock costs.
    ///
    /// **Not recovered, and there is nothing to recover it from on this class.**
    /// No field on `STUUnlock` or on any subclass is an amount of money. The
    /// money-adjacent fields are `m_00B16A0B`, which the reference tooling calls
    /// a Battle.net *product id* — a catalogue identifier, not a price — and
    /// `m_A7B393BF`/`m_A649130B`, which are a category and an unnamed enum.
    /// Prices live in store data this project does not read.
    ///
    /// The method exists so the gap is a documented `None` rather than a missing
    /// field a caller would have to go looking for.
    pub fn price(&self) -> Option<i32> {
        None
    }
}

/// Read one unlock out of an already-parsed asset.
///
/// The asset's **first** instance is taken as the unlock. That is not a guess:
/// an unlock asset is one record, and the reference tooling loads it as
/// `GetInstance<STU_3021DDED>(key)` (`Unlock.cs:113`). A type-`0xA5` asset whose
/// first instance is some unrelated class still produces a record, carrying
/// [`UnlockKind::Unknown`], rather than being skipped: an unrecognised shape is
/// a fact the survey should print, not hide.
pub fn read_unlock_in(stu: &mut Stu, asset: u64) -> Option<Unlock> {
    if stu.instances.is_empty() {
        return None;
    }
    let root = 0usize;
    let root_class = stu.instances[root].hash;

    let mut unlock = Unlock {
        asset,
        root_class,
        kind: unlock_kind_of(root_class),
        name: stu.read_asset_ref(root, field::unlock::NAME),
        description: stu.read_asset_ref(root, field::display::DESCRIPTION),
        available_in: stu.read_asset_ref(root, field::display::AVAILABLE_IN),
        rarity: stu
            .read_u32(root, field::display::RARITY)
            .map(|v| Rarity::from_raw(v as i32)),
        category: stu
            .read_u32(root, field::unlock::CATEGORY)
            .map(|v| Category::from_raw(v as i32)),
        categories: read_ref_array(stu, root, field::display::CATEGORIES),
        product_id: stu
            .read_u32(root, field::unlock::PRODUCT_ID)
            .map(|v| v as i64),
        resource_key: stu.read_asset_ref(root, field::unlock::RESOURCE_KEY),
        esports_team: stu.read_asset_ref(root, field::display::ESPORTS_TEAM),
        loot_box_kind: None,
        portrait_rank: None,
        level: None,
        stars: None,
        grants: Vec::new(),
        values: Vec::new(),
    };

    // Per-subclass fields. A field is read only where the reference tooling
    // declares it on *this* class: a hash that lives on some other class would
    // be read at this instance's offsets and produce noise that looks like data.
    {
        let mut grant = |stu: &mut Stu, field_hash: u32, label: &'static str| {
            if let Some(target) = stu.read_asset_ref(root, field_hash) {
                if target != 0 {
                    unlock.grants.push(Grant {
                        field: field_hash,
                        label: Some(label),
                        target,
                    });
                }
            }
        };
        let mut value = |stu: &mut Stu, field_hash: u32, label: &'static str| {
            if let Some(raw) = stu.read_u32(root, field_hash) {
                unlock.values.push((label, raw as i32));
            }
        };

        match root_class {
            class::UNLOCK_SKIN_THEME => {
                grant(stu, field::skin_unlock::SKIN_THEME, "m_skinTheme");
            }
            class::HERO => grant(stu, field::hero_unlock::HERO, "m_hero"),
            // The souvenir derives from the emote class, so it reads the emote's
            // fields too. The reference tooling's own switch does the same: the
            // field is inherited rather than redeclared, so the bytes are in the
            // payload at the same offset either way.
            class::UNLOCK_EMOTE | class::SOUVENIR => {
                grant(stu, field::emote::EMOTE, "m_emote");
            }
            class::UNLOCK_POSE => grant(stu, field::pose::POSE, "m_pose"),
            class::UNLOCK_VOICE_LINE => {
                grant(stu, field::voice_line::UNNAMED_F57B051E, "m_F57B051E");
            }
            class::UNLOCK_SPRAY_PAINT => {
                grant(stu, field::spray::UNNAMED_E29A79B7, "m_E29A79B7");
            }
            class::UNLOCK_POTG_ANIMATION => {
                grant(stu, field::potg::ANIMATION, "m_animation");
                grant(stu, field::potg::SKIN_THEME, "m_5530D275");
            }
            class::UNLOCK_PORTRAIT_FRAME => {
                grant(stu, field::portrait_frame::RANK_TEXTURE, "m_rankTexture");
                if let Some(raw) = stu.read_u32(root, field::portrait_frame::RANK) {
                    unlock.portrait_rank = Some(PortraitFrameRank::from_raw(raw as i32));
                }
                // `m_level` and `m_stars` are adjacent `ushort`s and the raw
                // layer has no sixteen-bit reader. Every container in this tree
                // is little-endian, so the pair is the low and high halves of one
                // `u32`. Stated here rather than left as a silent assumption: a
                // frame that ever reads a level above 65535 is this bug showing.
                if let Some(packed) = stu.read_u32(root, field::portrait_frame::LEVEL) {
                    unlock.level = Some((packed & 0xFFFF) as i32);
                    unlock.stars = Some((packed >> 16) as i32);
                }
            }
            class::UNLOCK_WEAPON => value(stu, field::skin_weapon::WEAPON_TYPE, "m_weaponType"),
            class::UNLOCK_LOOT_BOX => {
                grant(stu, field::loot_unlock::HERO, "m_3CC2088A");
                unlock.loot_box_kind = stu
                    .read_u32(root, field::loot_unlock::LOOT_BOX_TYPE)
                    .map(|v| LootBoxKind::from_raw(v as i32));
                // The box's own rarity, which `Unlock.cs:162` says replaces the
                // inherited `m_rarity`.
                if let Some(raw) = stu.read_u32(root, field::loot_unlock::RARITY) {
                    unlock.rarity = Some(Rarity::from_raw(raw as i32));
                }
            }
            class::UNLOCK_CURRENCY => value(stu, field::amount::CURRENCY, "m_currency"),
            class::UNLOCK_COMPETITIVE_CURRENCY => {
                value(stu, field::amount::COMPETITIVE_CURRENCY, "m_760BF18E")
            }
            class::UNLOCK_OWL_TOKEN => value(stu, field::amount::OWL_TOKEN, "m_63A026AF"),
            class::VIRTUAL_CURRENCY => value(stu, field::amount::VIRTUAL_CURRENCY, "m_amount"),
            class::BATTLE_PASS_XP => value(stu, field::amount::BATTLE_PASS_XP, "m_E0A45C1B"),
            class::BATTLE_PASS_TIER_SKIP => {
                value(stu, field::amount::BATTLE_PASS_TIER_SKIP, "m_D10618D1")
            }
            class::COMPETITIVE_SIGNATURE => {
                if let Some(raw) = stu.read_u32(root, field::amount::SIGNATURE_LEVEL) {
                    unlock.level = Some(raw as i32);
                }
            }
            class::SKIN_COMPONENT => {
                grant(stu, field::skin_component::PARENT_UNLOCK, "m_E2A492C7");
                grant(stu, field::skin_component::SLOT, "m_slot");
                grant(stu, field::skin_component::ITEM, "m_4EC40A19");
            }
            class::WEAPON_CHARM => {
                grant(stu, field::weapon_charm::UNNAMED_F1F888F9, "m_F1F888F9");
                grant(stu, field::weapon_charm::UNNAMED_06EE5924, "m_06EE5924");
            }
            class::NAME_CARD => {
                grant(stu, field::name_card::UNNAMED_C5B31BBA, "m_C5B31BBA");
                grant(stu, field::name_card::UNNAMED_0CAFC9BA, "m_0CAFC9BA");
            }
            class::STORY_MISSION => {
                grant(stu, field::story_mission::UNNAMED_216EA6DA, "m_216EA6DA")
            }
            class::WEAPON_SKIN => grant(stu, field::weapon_skin::UNNAMED_A9736011, "m_A9736011"),
            _ => {}
        }
    }

    Some(unlock)
}

// ---------------------------------------------------------------------------
// Skin themes
// ---------------------------------------------------------------------------

/// A model a skin theme swaps in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinModel {
    /// `m_model`: a model asset, type `0x00C`.
    pub model: u64,
    /// `m_2F08CD5E` element count. The elements are `STU_D9E3E761` records
    /// (`Misc_7.cs:7350`) whose only field is a reference this reader does not
    /// follow; the count is carried so the shape is visible without claiming to
    /// have read it.
    pub extra_count: u32,
}

/// A weapon slot a skin theme restyles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinWeapon {
    /// `m_BECFBDBA`: a `STU_649567C7`, itself an empty subclass of the skin
    /// base, so this is another skin asset rather than a model.
    pub skin: u64,
    /// `m_weaponType`, decoded.
    pub variation: WeaponVariation,
    /// `m_weaponType` as stored, so a caller can histogram the raw values rather
    /// than trust the decode. On the reference install the four slots of every
    /// sampled theme read 1, 2, 3 and 4 — golden, jade, galactic, crimson wolf.
    pub raw_variation: i32,
}

/// One customization slot of a mythic skin.
#[derive(Debug, Clone, PartialEq)]
pub struct MythicSlot {
    /// `m_id`, an `STUIdentifier` — a key, not an asset.
    pub id: Option<u64>,
    /// `m_displayText`, a display-text asset.
    pub display_text: Option<u64>,
    /// `m_texture`.
    pub texture: Option<u64>,
    /// `m_animation`.
    pub animation: Option<u64>,
    /// `m_57CE9041` element count: how many items the slot offers.
    pub item_count: u32,
}

/// A team-specific variant of a skin theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamVariant {
    /// `m_sourceTeam`, a `STUTeamColor`.
    pub source_team: Option<u64>,
    /// `m_generatedSkinTheme`, another skin theme.
    pub generated_theme: Option<u64>,
}

/// A skin theme, or a mythic skin: the asset that says what a cosmetic *is*.
///
/// This is the cosmetics root, and [`SkinTheme::models`] is the join the
/// renderer needs — a skin names the models it swaps in. It stops there: the
/// materials and body textures are not in this asset. See the module header and
/// `docs/formats/content.md` — "The skin join that is still missing".
#[derive(Debug, Clone, PartialEq)]
pub struct SkinTheme {
    pub asset: u64,
    /// `0x42270D59` for a plain theme, `0xEF85B312` for a mythic one.
    pub root_class: u32,
    pub is_mythic: bool,
    /// `m_internalName` — stored as text, not as a display-text asset, which
    /// makes it the only skin name readable without the text layer.
    pub internal_name: Option<String>,
    /// `m_ECCC4A5D`, the portrait the reference tooling saves under that name.
    pub portrait: Option<u64>,
    /// `m_BCC55571`: the models the skin swaps in.
    pub models: Vec<SkinModel>,
    /// `m_5FC3164C`: the per-weapon-slot restyles.
    pub weapon_skins: Vec<SkinWeapon>,
    /// `m_70D5F086`: team variants.
    pub team_variants: Vec<TeamVariant>,
    /// `m_teamColor`.
    pub team_color: Option<u64>,
    /// `m_50FDDF83`, a second `STUTeamColor`.
    pub unnamed_team_color: Option<u64>,
    /// `m_color`, a `teColorRGBA`.
    pub color: Option<[f32; 4]>,
    /// Mythic only: `m_hero`.
    pub hero: Option<u64>,
    /// Mythic only: `m_942A6CCA`, the customization slots.
    pub slots: Vec<MythicSlot>,
    /// Everything else this class carries, labelled by hash where the reference
    /// tooling has no name, so nothing read here is silently dropped.
    pub other_refs: Vec<Grant>,
}

/// Read a skin theme out of an already-parsed asset.
///
/// The skin instance is *found* rather than assumed to be first: a skin-theme
/// asset's root is `STUSkinTheme` or `STU_EF85B312`, and both are reachable
/// from nested structures, so a walk that took instance 0 blindly could land on
/// a nested instance instead.
pub fn read_skin_theme_in(stu: &mut Stu, asset: u64) -> Option<SkinTheme> {
    let root = stu
        .instances
        .iter()
        .position(|i| i.hash == class::SKIN_THEME || i.hash == class::MYTHIC_SKIN)?;
    let root_class = stu.instances[root].hash;
    let is_mythic = root_class == class::MYTHIC_SKIN;

    let mut theme = SkinTheme {
        asset,
        root_class,
        is_mythic,
        internal_name: stu.read_string(root, field::skin_theme::INTERNAL_NAME),
        portrait: stu.read_asset_ref(root, field::skin_theme::PORTRAIT),
        models: Vec::new(),
        weapon_skins: Vec::new(),
        team_variants: Vec::new(),
        team_color: stu.read_asset_ref(root, field::skin_theme::TEAM_COLOR),
        unnamed_team_color: stu.read_asset_ref(root, field::skin_base::UNNAMED_50FDDF83),
        color: stu.read_rgba(root, field::skin_theme::COLOR),
        hero: if is_mythic {
            stu.read_asset_ref(root, field::mythic::HERO)
        } else {
            None
        },
        slots: Vec::new(),
        other_refs: Vec::new(),
    };

    // The swapped-in models. Each element's layout is walked by hand rather than
    // through `read_asset_ref`, because that addresses the *instance's* payload
    // and an element payload starts somewhere else — the same trap `hero.rs`
    // documents for `asset_ref_at`.
    for (bag, start) in stu.inline_elements(root, field::skin_base::MODELS) {
        if bag < 0 {
            continue;
        }
        let layout = stu.layout_at(start, bag);
        let model = layout
            .iter()
            .find(|f| f.hash == field::skin_model::MODEL)
            .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash));
        let Some(model) = model else { continue };
        let extra_count = layout
            .iter()
            .find(|f| f.hash == field::skin_model::UNNAMED_2F08CD5E)
            .and_then(|f| stu.i32_at(start + f.offset))
            .filter(|n| (0..=4096).contains(n))
            .unwrap_or(0) as u32;
        theme.models.push(SkinModel { model, extra_count });
    }

    // The per-weapon-slot restyles.
    for (bag, start) in stu.inline_elements(root, field::skin_base::WEAPONS) {
        if bag < 0 {
            continue;
        }
        let layout = stu.layout_at(start, bag);
        let skin = layout
            .iter()
            .find(|f| f.hash == field::skin_weapon::SKIN)
            .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash));
        let Some(skin) = skin else { continue };
        let raw_variation = layout
            .iter()
            .find(|f| f.hash == field::skin_weapon::WEAPON_TYPE)
            .and_then(|f| stu.i32_at(start + f.offset))
            .unwrap_or(0);
        theme.weapon_skins.push(SkinWeapon {
            skin,
            variation: WeaponVariation::from_raw(raw_variation),
            raw_variation,
        });
    }

    // Team variants.
    for (bag, start) in stu.inline_elements(root, field::skin_theme::TEAM_VARIANTS) {
        if bag < 0 {
            continue;
        }
        let layout = stu.layout_at(start, bag);
        let pick = |hash: u32| {
            layout
                .iter()
                .find(|f| f.hash == hash)
                .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash))
        };
        theme.team_variants.push(TeamVariant {
            source_team: pick(field::team_variant::SOURCE_TEAM),
            generated_theme: pick(field::team_variant::GENERATED_THEME),
        });
    }

    // Mythic slots.
    if is_mythic {
        for (bag, start) in stu.inline_elements(root, field::mythic::SLOTS) {
            if bag < 0 {
                continue;
            }
            let layout = stu.layout_at(start, bag);
            let pick_ref = |hash: u32| {
                layout
                    .iter()
                    .find(|f| f.hash == hash)
                    .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash))
            };
            let item_count = layout
                .iter()
                .find(|f| f.hash == field::mythic_slot::ITEMS)
                .and_then(|f| stu.i32_at(start + f.offset))
                .filter(|n| (0..=4096).contains(n))
                .unwrap_or(0) as u32;
            theme.slots.push(MythicSlot {
                id: pick_ref(field::mythic_slot::ID),
                display_text: pick_ref(field::mythic_slot::DISPLAY_TEXT),
                texture: pick_ref(field::mythic_slot::TEXTURE),
                animation: pick_ref(field::mythic_slot::ANIMATION),
                item_count,
            });
        }
    }

    // The remaining single references, so nothing on the class goes unread.
    for (hash, label) in [
        (field::skin_theme::UNNAMED_E26643F0, "m_E26643F0"),
        (field::skin_theme::UNNAMED_02F889AE, "m_02F889AE"),
    ] {
        if let Some(target) = stu.read_asset_ref(root, hash) {
            if target != 0 {
                theme.other_refs.push(Grant {
                    field: hash,
                    label: Some(label),
                    target,
                });
            }
        }
    }
    theme.other_refs.extend(
        read_ref_array(stu, root, field::skin_theme::UNNAMED_71818BC5)
            .into_iter()
            .map(|target| Grant {
                field: field::skin_theme::UNNAMED_71818BC5,
                label: Some("m_71818BC5"),
                target,
            }),
    );

    Some(theme)
}

// ---------------------------------------------------------------------------
// Loot boxes
// ---------------------------------------------------------------------------

/// A loot box: what it looks like, and which theme it is.
///
/// **The drop pool is not here.** A box's pool is `STULootBoxUnlocks`, reached
/// from a progression-unlocks asset, and its rarity weights are
/// `STULootBoxRules`, reached from a generic-settings asset. Neither is a child
/// of this one; see [`LootBoxUnlocks`] and [`loot_box_rules`].
#[derive(Debug, Clone, PartialEq)]
pub struct LootBox {
    pub asset: u64,
    /// `m_lootBoxType`.
    pub kind: Option<LootBoxKind>,
    /// `m_modelLook` and `m_baseModelLook`. The closest thing a box has to a
    /// material reference, and it is a look rather than a skin.
    pub model_look: Option<u64>,
    pub base_model_look: Option<u64>,
    pub chest_entity: Option<u64>,
    pub base_entity: Option<u64>,
    pub idle_effect: Option<u64>,
    pub celebration: Option<u64>,
    pub name: Option<u64>,
    /// `m_shopCards` element count.
    pub shop_card_count: u32,
    pub hide_pucks: Option<u8>,
}

/// Read one loot-box asset.
pub fn read_loot_box_in(stu: &mut Stu, asset: u64) -> Option<LootBox> {
    let root = stu.instance(class::LOOT_BOX)?;
    Some(LootBox {
        asset,
        kind: stu
            .read_u32(root, field::loot_box::LOOT_BOX_TYPE)
            .map(|v| LootBoxKind::from_raw(v as i32)),
        model_look: stu.read_asset_ref(root, field::loot_box::MODEL_LOOK),
        base_model_look: stu.read_asset_ref(root, field::loot_box::BASE_MODEL_LOOK),
        chest_entity: stu.read_asset_ref(root, field::loot_box::CHEST_ENTITY),
        base_entity: stu.read_asset_ref(root, field::loot_box::BASE_ENTITY),
        idle_effect: stu.read_asset_ref(root, field::loot_box::IDLE_EFFECT),
        celebration: stu.read_asset_ref(root, field::loot_box::CELEBRATION),
        name: stu.read_asset_ref(root, field::loot_box::NAME),
        shop_card_count: inline_count(stu, root, field::loot_box::SHOP_CARDS),
        hide_pucks: stu
            .read_u32(root, field::loot_box::HIDE_PUCKS)
            .map(|v| v as u8),
    })
}

/// One rarity's drop weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RarityRule {
    pub rarity: Rarity,
    /// `m_A07D6F99`. The reference tooling never reads this field, so what the
    /// number weighs is **not** established. It is carried exactly as stored,
    /// and this type calls it "a weight" only because it sits beside `m_rarity`
    /// in a class named `RarityRules`.
    pub weight_a: u32,
    /// `m_CBE06580`, likewise.
    pub weight_b: u32,
}

/// A loot box's rarity table.
#[derive(Debug, Clone, PartialEq)]
pub struct LootBoxRules {
    /// The settings asset the rules were found in.
    pub asset: u64,
    pub kind: Option<LootBoxKind>,
    pub rarity_rules: Vec<RarityRule>,
    pub currency_rarity_rules: Vec<RarityRule>,
    pub unnamed_24391f9b: Option<i32>,
    pub unnamed_53b8c818: Option<i32>,
}

/// A loot box's drop pool: the unlocks it can yield.
#[derive(Debug, Clone, PartialEq)]
pub struct LootBoxUnlocks {
    pub kind: Option<LootBoxKind>,
    pub unlocks: Vec<u64>,
}

/// Find the loot-box rarity rules in the install.
///
/// They are not in a loot-box asset. They live in the generic-settings asset
/// that carries `STUGenericSettings_LootBoxes` (`0x9044BCFE`), as declared at
/// `STUGenericSettings_LootBoxes.cs:6`. Which asset that is is not fixed per
/// install, so this walks every generic-settings asset and returns everything it
/// finds, tagged with the asset it came from.
pub fn loot_box_rules(data: &GameData) -> Vec<LootBoxRules> {
    let mut out = Vec::new();
    for asset in data.guids_of_type(crate::guid::types::STU_GENERIC_SETTINGS) {
        let Ok(mut stu) = data.stu_of(asset) else {
            continue;
        };
        let Some(settings) = stu.instance(class::GENERIC_SETTINGS_LOOT_BOXES) else {
            continue;
        };
        for (bag, start) in stu.inline_elements(settings, field::loot_settings::RULES) {
            if bag < 0 {
                continue;
            }
            out.push(read_loot_box_rules_at(&mut stu, asset, start, bag));
        }
    }
    out
}

/// Read one `STULootBoxRules` whose payload starts at `start`.
fn read_loot_box_rules_at(stu: &mut Stu, asset: u64, start: usize, bag: i32) -> LootBoxRules {
    let layout = stu.layout_at(start, bag);
    // The scalar fields are read first: `read_rarity_rules` needs `&mut Stu`,
    // and a closure over `&Stu` would still be borrowing when that runs.
    let scalars = [
        field::loot_rules::LOOT_BOX_TYPE,
        field::loot_rules::UNNAMED_24391F9B,
        field::loot_rules::UNNAMED_53B8C818,
    ]
    .map(|hash| {
        layout
            .iter()
            .find(|f| f.hash == hash)
            .and_then(|f| stu.i32_at(start + f.offset))
    });
    let [kind, unnamed_24391f9b, unnamed_53b8c818] = scalars;
    LootBoxRules {
        asset,
        kind: kind.map(LootBoxKind::from_raw),
        rarity_rules: read_rarity_rules(stu, start, &layout, field::loot_rules::RARITY_RULES),
        currency_rarity_rules: read_rarity_rules(
            stu,
            start,
            &layout,
            field::loot_rules::CURRENCY_RARITY_RULES,
        ),
        unnamed_24391f9b,
        unnamed_53b8c818,
    }
}

/// Read a `STULootBoxRarityRules` inline array that starts inside a payload.
fn read_rarity_rules(
    stu: &mut Stu,
    payload_start: usize,
    layout: &[ow_stu::FieldInfo],
    field_hash: u32,
) -> Vec<RarityRule> {
    // An inline instance array: an `i32` count at the field, then that many
    // payloads, each starting with its own field-bag index. This is the shape
    // `Stu::inline_elements` walks, spelled out because the element fields are
    // read here and the bag index has to come from the same place it does there.
    let Some(offset) = layout
        .iter()
        .find(|f| f.hash == field_hash)
        .map(|f| f.offset)
    else {
        return Vec::new();
    };
    let array_at = payload_start + offset;
    let Some(count) = stu.i32_at(array_at) else {
        return Vec::new();
    };
    if !(0..=4096).contains(&count) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut at = array_at + 4;
    for _ in 0..count {
        let Some(bag) = stu.i32_at(at) else { break };
        if bag < 0 {
            break;
        }
        let element = stu.layout_at(at, bag);
        let u32_field = |hash: u32| {
            element
                .iter()
                .find(|f| f.hash == hash)
                .and_then(|f| stu.u32_at(at + f.offset))
        };
        out.push(RarityRule {
            rarity: u32_field(field::rarity_rule::RARITY)
                .map(|v| Rarity::from_raw(v as i32))
                .unwrap_or(Rarity::Unknown(0)),
            weight_a: u32_field(field::rarity_rule::WEIGHT_A).unwrap_or(0),
            weight_b: u32_field(field::rarity_rule::WEIGHT_B).unwrap_or(0),
        });
        at += element
            .last()
            .map(|f| f.offset + f.size)
            .unwrap_or(4)
            .max(4);
    }
    out
}

// ---------------------------------------------------------------------------
// Achievements
// ---------------------------------------------------------------------------

/// An achievement: what it is called, what it grants, and how far it counts.
#[derive(Debug, Clone, PartialEq)]
pub struct Achievement {
    pub asset: u64,
    pub name: Option<u64>,
    pub description: Option<u64>,
    /// `m_245A3F6D`, the icon texture.
    pub icon: Option<u64>,
    /// `m_544A6A4F`, a second texture of unestablished purpose.
    pub other_texture: Option<u64>,
    /// `m_290B2ADF`, unnamed.
    pub unnamed_290b2adf: Option<u64>,
    /// `m_unlock`, the reward.
    pub unlock: Option<u64>,
    /// `m_4E291DCC`, stored as text rather than as a display-text asset.
    pub string: Option<String>,
    /// `m_value`, the progress target.
    pub value: Option<i32>,
    pub unnamed_e1f5c73d: Option<i32>,
    pub unnamed_628d48cc: Option<i32>,
    /// `m_5351832E`; the generated default is -1, so a stored -1 means "not
    /// authored" rather than "minus one".
    pub unnamed_5351832e: Option<i32>,
    /// How many config-var wrappers this achievement carries. Counted, not
    /// decoded: the number lives inside them, and following that is a separate
    /// piece of work.
    pub config_var_count: u32,
}

/// Read one achievement asset.
pub fn read_achievement_in(stu: &mut Stu, asset: u64) -> Option<Achievement> {
    let root = stu.instance(class::ACHIEVEMENT)?;
    let config_var_count: u32 = field::achievement::CONFIG_VAR_FIELDS
        .iter()
        .map(|hash| inline_count(stu, root, *hash))
        .sum();
    Some(Achievement {
        asset,
        name: stu.read_asset_ref(root, field::achievement::NAME),
        description: stu.read_asset_ref(root, field::achievement::DESCRIPTION),
        icon: stu.read_asset_ref(root, field::achievement::ICON),
        other_texture: stu.read_asset_ref(root, field::achievement::UNNAMED_544A6A4F),
        unnamed_290b2adf: stu.read_asset_ref(root, field::achievement::UNNAMED_290B2ADF),
        unlock: stu.read_asset_ref(root, field::achievement::UNLOCK),
        string: stu.read_string(root, field::achievement::STRING),
        value: stu
            .read_u32(root, field::achievement::VALUE)
            .map(|v| v as i32),
        unnamed_e1f5c73d: stu
            .read_u32(root, field::achievement::UNNAMED_E1F5C73D)
            .map(|v| v as i32),
        unnamed_628d48cc: stu
            .read_u32(root, field::achievement::UNNAMED_628D48CC)
            .map(|v| v as i32),
        unnamed_5351832e: stu
            .read_u32(root, field::achievement::UNNAMED_5351832E)
            .map(|v| v as i32),
        config_var_count,
    })
}

// ---------------------------------------------------------------------------
// Ranked seasons
// ---------------------------------------------------------------------------

/// A ranked season.
///
/// `teStructuredDataDateAndTime` is eight bytes on this build and this reader
/// does not decode it, so the two date fields are carried as raw bytes rather
/// than as a date. Decoding them is an open item, not a value; see
/// `docs/formats/content.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedSeason {
    pub asset: u64,
    /// `m_rankedSeason`, the season number.
    pub season: Option<i32>,
    /// `m_58066D8F`, the season's rewards.
    pub unlocks: Vec<u64>,
    /// `m_heroicUnlocks`.
    pub heroic_unlocks: Vec<u64>,
    pub unnamed_503baa0a: Option<f32>,
    pub unnamed_85059b03: Option<u32>,
    pub unnamed_82aa08b4: Option<i32>,
    pub unnamed_46b0b905: Option<i32>,
    pub unnamed_842af60f: Option<i32>,
    pub unnamed_06122648: Option<i32>,
    /// `m_F82A8727`, raw bytes. See the type comment.
    pub start_date_raw: Option<[u8; 8]>,
    /// `m_275B6D5C`, raw bytes.
    pub end_date_raw: Option<[u8; 8]>,
}

/// Read one ranked-season asset.
pub fn read_ranked_season_in(stu: &mut Stu, asset: u64) -> Option<RankedSeason> {
    let root = stu.instance(class::RANKED_SEASON)?;
    // The date fields are read as a `u64` and taken back to bytes: the raw layer
    // has no `u8` accessor, and little-endian means those eight bytes are the
    // same eight bytes the `u64` was read from. Both are read up front, because
    // resolving a field needs `&mut Stu`.
    let date = |stu: &mut Stu, hash: u32| {
        let (offset, size) = stu.field_bytes(root, hash)?;
        if size < 8 {
            return None;
        }
        stu.u64_at(stu.payload_start(root) + offset)
            .map(|v| v.to_le_bytes())
    };
    let start_date_raw = date(stu, field::ranked_data::START_DATE);
    let end_date_raw = date(stu, field::ranked_data::END_DATE);
    Some(RankedSeason {
        asset,
        season: stu
            .read_u32(root, field::ranked_season::SEASON)
            .map(|v| v as i32),
        unlocks: unlocks_at_field(stu, root, field::ranked_data::UNLOCKS),
        heroic_unlocks: unlocks_at_field(stu, root, field::ranked_data::HEROIC_UNLOCKS),
        unnamed_503baa0a: stu.read_f32(root, field::ranked_data::UNNAMED_503BAA0A),
        unnamed_85059b03: stu.read_u32(root, field::ranked_data::UNNAMED_85059B03),
        unnamed_82aa08b4: stu
            .read_u32(root, field::ranked_data::UNNAMED_82AA08B4)
            .map(|v| v as i32),
        unnamed_46b0b905: stu
            .read_u32(root, field::ranked_data::UNNAMED_46B0B905)
            .map(|v| v as i32),
        unnamed_842af60f: stu
            .read_u32(root, field::ranked_data::UNNAMED_842AF60F)
            .map(|v| v as i32),
        unnamed_06122648: stu
            .read_u32(root, field::ranked_data::UNNAMED_06122648)
            .map(|v| v as i32),
        start_date_raw,
        end_date_raw,
    })
}

// ---------------------------------------------------------------------------
// Progression unlocks
// ---------------------------------------------------------------------------

/// The unlocks one progression level grants.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressionLevel {
    pub level: Option<i32>,
    pub unlocks: Vec<u64>,
}

/// A progression-unlocks asset: the reward track.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressionUnlocks {
    pub asset: u64,
    /// `m_9135A4B2`.
    pub unlocks: Vec<u64>,
    /// `m_otherUnlocks`.
    pub other_unlocks: Vec<u64>,
    /// `m_7846C401`: per-level rewards.
    pub levels: Vec<ProgressionLevel>,
    /// `m_lootBoxesUnlocks`: per-box drop pools.
    pub loot_boxes: Vec<LootBoxUnlocks>,
    /// `m_160B0466`, another progression-unlocks asset.
    pub parent: Option<u64>,
}

/// Read one progression-unlocks asset.
pub fn read_progression_in(stu: &mut Stu, asset: u64) -> Option<ProgressionUnlocks> {
    let root = stu.instance(class::PROGRESSION_UNLOCKS)?;

    let mut levels = Vec::new();
    for (bag, start) in stu.inline_elements(root, field::progression::LEVELS) {
        if bag < 0 {
            continue;
        }
        let layout = stu.layout_at(start, bag);
        let level = layout
            .iter()
            .find(|f| f.hash == field::progression_level::LEVEL)
            .and_then(|f| stu.i32_at(start + f.offset));
        let unlocks = layout
            .iter()
            .find(|f| f.hash == field::progression_level::UNLOCKS)
            .map(|f| inline_instance_unlocks(stu, start + f.offset))
            .unwrap_or_default();
        levels.push(ProgressionLevel { level, unlocks });
    }

    let mut loot_boxes = Vec::new();
    for (bag, start) in stu.inline_elements(root, field::progression::LOOT_BOXES) {
        if bag < 0 {
            continue;
        }
        let layout = stu.layout_at(start, bag);
        let kind = layout
            .iter()
            .find(|f| f.hash == field::loot_unlocks::LOOT_BOX_TYPE)
            .and_then(|f| stu.i32_at(start + f.offset))
            .map(LootBoxKind::from_raw);
        let unlocks = layout
            .iter()
            .find(|f| f.hash == field::loot_unlocks::UNLOCKS)
            .map(|f| inline_instance_unlocks(stu, start + f.offset))
            .unwrap_or_default();
        loot_boxes.push(LootBoxUnlocks { kind, unlocks });
    }

    Some(ProgressionUnlocks {
        asset,
        unlocks: unlocks_at_field(stu, root, field::progression::UNLOCKS),
        other_unlocks: unlocks_at_field(stu, root, field::progression::OTHER_UNLOCKS),
        levels,
        loot_boxes,
        parent: stu.read_asset_ref(root, field::progression::PARENT),
    })
}

// ---------------------------------------------------------------------------
// Voice conversations
// ---------------------------------------------------------------------------

/// One line of a voice conversation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConversationLine {
    /// `m_E295B99C`, the `STUVoiceLine` (type `0x70`).
    pub voice_line: u64,
    /// `m_B4D405A1`, carried as stored: the reference tooling does not read it.
    pub unnamed_b4d405a1: u32,
}

/// A voice conversation: what a hero says in response to a stimulus.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceConversation {
    pub asset: u64,
    /// `m_stimulus`, the `STUVoiceStimulus` this answers.
    pub stimulus: Option<u64>,
    /// `m_90D76F17`, the lines.
    pub lines: Vec<ConversationLine>,
    /// `m_weight`; the generated default is 1.0f.
    pub weight: Option<f32>,
    /// `m_265A80ED`, an unnamed `ushort[]` in the dynamic block. What the
    /// numbers mean is not established; they are carried as read.
    pub unnamed_265a80ed: Vec<u16>,
}

/// Read one voice-conversation asset.
pub fn read_conversation_in(stu: &mut Stu, asset: u64) -> Option<VoiceConversation> {
    let root = stu.instance(class::VOICE_CONVERSATION)?;
    let mut lines = Vec::new();
    for (bag, start) in stu.inline_elements(root, field::conversation::LINES) {
        if bag < 0 {
            continue;
        }
        let layout = stu.layout_at(start, bag);
        let voice_line = layout
            .iter()
            .find(|f| f.hash == field::conversation_line::VOICE_LINE)
            .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash));
        let Some(voice_line) = voice_line else {
            continue;
        };
        lines.push(ConversationLine {
            voice_line,
            unnamed_b4d405a1: layout
                .iter()
                .find(|f| f.hash == field::conversation_line::UNNAMED_B4D405A1)
                .and_then(|f| stu.u32_at(start + f.offset))
                .unwrap_or(0),
        });
    }
    Some(VoiceConversation {
        asset,
        stimulus: stu.read_asset_ref(root, field::conversation::STIMULUS),
        lines,
        weight: stu.read_f32(root, field::conversation::WEIGHT),
        unnamed_265a80ed: read_u16_array(stu, root, field::conversation::UNNAMED_265A80ED),
    })
}

// ---------------------------------------------------------------------------
// Stats and catalogs
// ---------------------------------------------------------------------------

/// A stat definition: which heroes and maps a tracked number applies to.
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub asset: u64,
    pub name: Option<u64>,
    pub unnamed_9f574e87: Option<u64>,
    pub heroes: Vec<u64>,
    pub maps: Vec<u64>,
    pub teams: Vec<i32>,
    pub display_type: Option<u32>,
    pub category: Option<u32>,
    pub stat_type: Option<u32>,
    pub internal_only: Option<u8>,
}

/// Read one stat asset.
pub fn read_stat_in(stu: &mut Stu, asset: u64) -> Option<Stat> {
    let root = stu.instance(class::STAT)?;
    Some(Stat {
        asset,
        name: stu.read_asset_ref(root, field::stat::NAME),
        unnamed_9f574e87: stu.read_asset_ref(root, field::stat::UNNAMED_9F574E87),
        heroes: read_ref_array(stu, root, field::stat::HEROES),
        maps: read_ref_array(stu, root, field::stat::MAPS),
        teams: read_i32_array(stu, root, field::stat::TEAMS),
        display_type: stu.read_u32(root, field::stat::DISPLAY_TYPE),
        category: stu.read_u32(root, field::stat::CATEGORY),
        stat_type: stu.read_u32(root, field::stat::TYPE),
        internal_only: stu
            .read_u32(root, field::stat::INTERNAL_ONLY)
            .map(|v| v as u8),
    })
}

/// A catalog: a flat list of asset GUIDs.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub asset: u64,
    /// `m_assets`. Read as stored: this is a bare `ulong[]`, not a
    /// `teStructuredDataAssetRef[]`, so there is no per-element field hash to
    /// obfuscate with. Whether the values are therefore plaintext GUIDs is
    /// *measured* by `tests/unlocks_real.rs`, not assumed here.
    pub assets: Vec<u64>,
    /// `m_B7FC20A3`, a bare `long[]`.
    pub unnamed_b7fc20a3: Vec<i64>,
}

/// Read one catalog asset.
pub fn read_catalog_in(stu: &mut Stu, asset: u64) -> Option<Catalog> {
    let root = stu.instance(class::CATALOG)?;
    Some(Catalog {
        asset,
        assets: read_u64_array(stu, root, field::catalog::ASSETS),
        unnamed_b7fc20a3: read_i64_array(stu, root, field::catalog::UNNAMED_B7FC20A3),
    })
}

// ---------------------------------------------------------------------------
// Survey: the type_histogram analogue for the content surface
// ---------------------------------------------------------------------------

/// One readable record, whatever surface it came from.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Unlock(Box<Unlock>),
    SkinTheme(Box<SkinTheme>),
    LootBox(Box<LootBox>),
    Achievement(Box<Achievement>),
    RankedSeason(Box<RankedSeason>),
    ProgressionUnlocks(Box<ProgressionUnlocks>),
    VoiceConversation(Box<VoiceConversation>),
    Stat(Box<Stat>),
    Catalog(Box<Catalog>),
    /// The asset parsed, and its first instance is a class this reader has no
    /// name for. Recorded rather than skipped: "there is a shape here nobody has
    /// read" is exactly what a survey exists to report.
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
            Record::Unlock(r) => r.asset,
            Record::SkinTheme(r) => r.asset,
            Record::LootBox(r) => r.asset,
            Record::Achievement(r) => r.asset,
            Record::RankedSeason(r) => r.asset,
            Record::ProgressionUnlocks(r) => r.asset,
            Record::VoiceConversation(r) => r.asset,
            Record::Stat(r) => r.asset,
            Record::Catalog(r) => r.asset,
            Record::Unnamed { asset, .. } => *asset,
        }
    }

    /// The root instance class, for the surfaces where it is the identity of the
    /// record. Zero where the class is implied by the asset type.
    pub fn root_class(&self) -> u32 {
        match self {
            Record::Unlock(r) => r.root_class,
            Record::SkinTheme(r) => r.root_class,
            Record::Unnamed { root_class, .. } => *root_class,
            _ => 0,
        }
    }

    /// A one-line summary, for the CLI.
    ///
    /// This is a summary and not a dump: it names what the reader recovered so a
    /// census is readable at a glance. Every number in it is read from the
    /// asset; nothing here is inferred.
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
            Record::Unlock(r) => format!(
                "{:<20} class 0x{:08X} rarity={} category={} name={} grants=[{}] product_id={}",
                r.kind.to_string(),
                r.root_class,
                r.rarity.map(|v| v.to_string()).unwrap_or("unset".into()),
                r.category.map(|v| v.to_string()).unwrap_or("unset".into()),
                guid_or_none(r.name),
                r.grants
                    .iter()
                    .map(|g| g.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
                r.product_id
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".into()),
            ),
            Record::SkinTheme(r) => format!(
                "{:<11} class 0x{:08X} name={:?} models=[{}] weapon_skins=[{}] portrait={} slots={} color={:?}",
                if r.is_mythic { "mythic skin" } else { "skin theme" },
                r.root_class,
                r.internal_name,
                r.models
                    .iter()
                    .map(|m| format!("0x{:016x}", m.model))
                    .collect::<Vec<_>>()
                    .join(","),
                r.weapon_skins
                    .iter()
                    .map(|w| format!("0x{:016x}/{}", w.skin, w.variation))
                    .collect::<Vec<_>>()
                    .join(","),
                guid_or_none(r.portrait),
                r.slots.len(),
                r.color,
            ),
            Record::LootBox(r) => format!(
                "loot box       kind={} look={} base_look={} chest_entity={} shop_cards={}",
                r.kind.map(|v| v.to_string()).unwrap_or("unset".into()),
                guid_or_none(r.model_look),
                guid_or_none(r.base_model_look),
                guid_or_none(r.chest_entity),
                r.shop_card_count,
            ),
            Record::Achievement(r) => format!(
                "achievement     name={} value={:?} unlock={} icon={} text={:?} literals={}",
                guid_or_none(r.name),
                r.value,
                guid_or_none(r.unlock),
                guid_or_none(r.icon),
                r.string,
                r.config_var_count,
            ),
            Record::RankedSeason(r) => format!(
                "ranked season   season={:?} unlocks={} heroic={} raw_dates={}/{}",
                r.season,
                r.unlocks.len(),
                r.heroic_unlocks.len(),
                r.start_date_raw.is_some(),
                r.end_date_raw.is_some(),
            ),
            Record::ProgressionUnlocks(r) => format!(
                "progression     unlocks={} other={} levels={} loot_box_pools={} parent={}",
                r.unlocks.len(),
                r.other_unlocks.len(),
                r.levels.len(),
                r.loot_boxes.len(),
                guid_or_none(r.parent),
            ),
            Record::VoiceConversation(r) => format!(
                "conversation    stimulus={} lines=[{}] weight={:?} ushorts={}",
                guid_or_none(r.stimulus),
                guid_list(
                    &r.lines
                        .iter()
                        .map(|l| l.voice_line)
                        .collect::<Vec<u64>>()
                ),
                r.weight,
                r.unnamed_265a80ed.len(),
            ),
            Record::Stat(r) => format!(
                "stat            name={} heroes=[{}] maps={} teams={} display_type={:?} type={:?}",
                guid_or_none(r.name),
                guid_list(&r.heroes),
                r.maps.len(),
                r.teams.len(),
                r.display_type,
                r.stat_type,
            ),
            Record::Catalog(r) => format!(
                "catalog         assets={} longs={}",
                r.assets.len(),
                r.unnamed_b7fc20a3.len()
            ),
            Record::Unnamed {
                root_class,
                instances,
                ..
            } => format!("UNNAMED         root class 0x{root_class:08X}, {instances} instance(s)"),
        }
    }
}

/// A census of one asset type across the install.
///
/// This is the content-surface analogue of
/// [`GameData::type_histogram`](crate::GameData::type_histogram): counts first,
/// then a bounded sample of records. `records` holds at most `limit` entries
/// while every counter describes **all** the assets of the type, so a limit
/// narrows what is printed without narrowing what is measured.
#[derive(Debug, Default)]
pub struct Survey {
    pub type_id: u16,
    /// Assets of this type in the manifest registry.
    pub assets: usize,
    /// Assets whose bytes opened.
    pub readable: usize,
    /// Assets whose bytes did not open — in the registry but not downloaded.
    pub unreadable: usize,
    /// Assets that opened but did not parse as STU.
    pub unparsed: usize,
    /// Assets that parsed as version 1. A version 1 asset has no per-instance
    /// field bag, so its record would read as empty rather than wrong; the
    /// counter is here so that would be visible rather than silent.
    pub version_1: usize,
    /// Total instances across every parsed asset.
    pub instances: usize,
    /// First-instance class to number of assets. This is what a survey is for:
    /// it says which shapes actually shipped.
    pub root_classes: BTreeMap<u32, usize>,
    /// Assets whose first instance is a class no reader here claims.
    pub unrecognised: usize,
    /// A bounded sample of records.
    pub records: Vec<Record>,
    /// The first few parse errors, verbatim.
    pub errors: Vec<String>,
    /// Assets this survey actually opened. Equal to `assets` unless the survey
    /// was capped, in which case the counters describe a stride-sample of the
    /// type rather than all of it — which is why `capped` is recorded.
    pub scanned: usize,
    /// Whether `scanned` is less than `assets`.
    pub capped: bool,
}

impl Survey {
    /// Assets that opened and parsed, within what was scanned.
    pub fn parsed(&self) -> usize {
        self.readable - self.unparsed
    }

    /// How many sample records are typed rather than [`Record::Unnamed`].
    pub fn typed_in_sample(&self) -> usize {
        self.records
            .iter()
            .filter(|r| !matches!(r, Record::Unnamed { .. }))
            .count()
    }
}

/// Walk one asset type across the install and report what is there.
pub fn survey_type(data: &GameData, type_id: u16, limit: usize) -> Survey {
    survey_type_capped(data, type_id, limit, usize::MAX)
}

/// [`survey_type`] over at most `cap` assets, striding over the rest.
///
/// The cap exists because the reference install records **19,122** unlock assets
/// and downloads none of them: opening all of them is a CASC miss each and the
/// survey takes minutes. `scanned` and `capped` say what happened, so a capped
/// census is never mistaken for a complete one.
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
                continue;
            }
        };
        survey.readable += 1;
        let mut stu = match Stu::parse(bytes) {
            Ok(stu) => stu,
            Err(e) => {
                survey.unparsed += 1;
                if survey.errors.len() < 8 {
                    survey.errors.push(format!("0x{asset:016x}: {e}"));
                }
                continue;
            }
        };
        if stu.is_v1() {
            survey.version_1 += 1;
        }
        survey.instances += stu.instances.len();
        let Some(root) = stu.instances.first().map(|i| i.hash) else {
            survey.unrecognised += 1;
            continue;
        };
        *survey.root_classes.entry(root).or_insert(0) += 1;

        let record = record_for(type_id, &mut stu, asset);
        if matches!(record, Record::Unnamed { .. }) {
            survey.unrecognised += 1;
        }
        if survey.records.len() < limit {
            survey.records.push(record);
        }
    }
    survey
}

/// Every content type, surveyed over at most `cap` assets each. `limit` is the
/// record sample.
pub fn survey_content_capped(data: &GameData, limit: usize, cap: usize) -> Vec<Survey> {
    types::ALL
        .iter()
        .map(|t| survey_type_capped(data, *t, limit, cap))
        .collect()
}

/// Every content type, surveyed in full.
pub fn survey_content(data: &GameData, limit: usize) -> Vec<Survey> {
    survey_content_capped(data, limit, usize::MAX)
}

/// The reader for one asset type, or [`Record::Unnamed`] when the type has no
/// reader yet.
///
/// `0x103`, customizable skin themes, shares [`read_skin_theme_in`]. That is
/// measured rather than assumed: on the reference install all 31 readable assets
/// of type `0x103` carry root class `0xEF85B312`, the same `STU_EF85B312` the
/// type-`0xA6` mythic skins carry, and every one of them reads. See
/// `docs/formats/content.md`.
fn record_for(type_id: u16, stu: &mut Stu, asset: u64) -> Record {
    match type_id {
        types::UNLOCK => match read_unlock_in(stu, asset) {
            Some(r) => Record::Unlock(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::SKIN_THEME | types::CUSTOMIZABLE_SKIN_THEME => {
            match read_skin_theme_in(stu, asset) {
                Some(r) => Record::SkinTheme(Box::new(r)),
                None => unnamed(stu, asset),
            }
        }
        types::LOOT_BOX => match read_loot_box_in(stu, asset) {
            Some(r) => Record::LootBox(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::ACHIEVEMENT => match read_achievement_in(stu, asset) {
            Some(r) => Record::Achievement(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::RANKED_SEASON => match read_ranked_season_in(stu, asset) {
            Some(r) => Record::RankedSeason(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::PROGRESSION_UNLOCKS => match read_progression_in(stu, asset) {
            Some(r) => Record::ProgressionUnlocks(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::VOICE_CONVERSATION => match read_conversation_in(stu, asset) {
            Some(r) => Record::VoiceConversation(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::STAT => match read_stat_in(stu, asset) {
            Some(r) => Record::Stat(Box::new(r)),
            None => unnamed(stu, asset),
        },
        types::CATALOG => match read_catalog_in(stu, asset) {
            Some(r) => Record::Catalog(Box::new(r)),
            None => unnamed(stu, asset),
        },
        _ => unnamed(stu, asset),
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
// Whole-type readers, for callers that want the surface rather than a sample
// ---------------------------------------------------------------------------

/// Every loot-box asset.
pub fn loot_boxes(data: &GameData) -> Vec<LootBox> {
    read_type(data, types::LOOT_BOX, read_loot_box_in)
}

/// Every achievement asset.
pub fn achievements(data: &GameData) -> Vec<Achievement> {
    read_type(data, types::ACHIEVEMENT, read_achievement_in)
}

/// Every stat asset.
pub fn stats(data: &GameData) -> Vec<Stat> {
    read_type(data, types::STAT, read_stat_in)
}

/// Every catalog asset.
pub fn catalogs(data: &GameData) -> Vec<Catalog> {
    read_type(data, types::CATALOG, read_catalog_in)
}

// ---------------------------------------------------------------------------
// Low-level helpers the raw layer does not have
// ---------------------------------------------------------------------------

/// Walk every asset of `type_id`, parse it, and hand it to `read`.
///
/// Assets that do not open or do not parse are skipped, matching
/// `settings::read_all`: not every asset of a type is present in the loaded
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

/// How many elements an inline instance array has, or zero.
///
/// `Stu::inline_elements` already walks the array; this is the count of what it
/// found. A field that is not an inline array yields zero rather than an error,
/// so a caller counting three optional arrays does not have to tell the cases
/// apart.
fn inline_count(stu: &mut Stu, instance: usize, field_hash: u32) -> u32 {
    stu.inline_elements(instance, field_hash).len() as u32
}

/// The unlock list of an inline `STUUnlocks` whose payload starts at `at`.
///
/// This is the shape the generated classes call
/// `InlineInstanceFieldReader`: the field *is* the instance, so the first four
/// bytes at `at` are its field-bag index, not an element count. Reading it as a
/// count is the mistake that made `m_otherUnlocks` come back empty while the
/// typed layer read it fine — `docs/formats/content.md` records the measurement.
///
/// An **array** of these — `STU_1757E817[]`, `STULootBoxUnlocks[]` — is the other
/// shape, an `i32` count then that many payloads, and that is what
/// [`Stu::inline_elements`] walks; the callers here use it for those and come
/// to this function for the elements it returns.
///
/// From here the list is one more hop: `STUUnlocks.m_unlocks` is a reference
/// array in the dynamic block, so the GUIDs come out of [`dyn_ref_array_at`].
fn inline_instance_unlocks(stu: &mut Stu, at: usize) -> Vec<u64> {
    let Some(bag) = stu.i32_at(at) else {
        return Vec::new();
    };
    if bag < 0 {
        return Vec::new();
    }
    let layout = stu.layout_at(at, bag);
    let Some(offset) = layout
        .iter()
        .find(|f| f.hash == field::unlocks::UNLOCKS)
        .map(|f| f.offset)
    else {
        return Vec::new();
    };
    dyn_ref_array_at(stu, at + offset, field::unlocks::UNLOCKS)
}

/// The unlock list a field on `instance` names, where that field is an inline
/// `STUUnlocks`.
fn unlocks_at_field(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u64> {
    let Some((offset, _size)) = stu.field_bytes(instance, field_hash) else {
        return Vec::new();
    };
    inline_instance_unlocks(stu, stu.payload_start(instance) + offset)
}

/// A `teStructuredDataAssetRef<T>[]` field, read out of the dynamic block.
///
/// Element layout is the typed layer's: sixteen bytes per element with the GUID
/// at `+8`, deobfuscated with the *array's* field hash. Both facts are copied
/// from `ow_stu/src/types.rs:1030` rather than re-derived, and
/// `tests/unlocks_real.rs` checks the result by requiring the GUIDs to resolve
/// in the asset registry — a wrong stride yields values that resolve to nothing,
/// which is how the wrong layout was caught the first time it mattered.
fn read_ref_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u64> {
    let Some((offset, _size)) = stu.field_bytes(instance, field_hash) else {
        return Vec::new();
    };
    dyn_ref_array_at(stu, stu.payload_start(instance) + offset, field_hash)
}

/// The same, from an absolute offset rather than an instance and a field.
fn dyn_ref_array_at(stu: &mut Stu, at: usize, field_hash: u32) -> Vec<u64> {
    let Some(span) = dyn_array_span(stu, at) else {
        return Vec::new();
    };
    let (start, count) = span;
    let dyn_block = stu.dyn_data();
    let checksum = stu.header_checksum;
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let Some(stored) = le_u64(dyn_block, start + k * 16 + 8) else {
            break;
        };
        out.push(ow_stu::deobfuscate(checksum, field_hash, stored));
    }
    out
}

/// Where a dynamic-block array's elements are, as an offset into
/// [`Stu::dyn_data`] and a count.
///
/// The raw STU reader exposes the dynamic block but has no array reader, and
/// every list on this surface needs one. The descriptor shape is the typed
/// layer's `read_array` (`ow_stu/src/types.rs:894`): the field stores an `i32`
/// offset into the block, and the block at that offset holds `i32 count`, a
/// `u32` pad, and an `i64` element offset relative to the block. What lives in
/// an element is the caller's business, because a
/// `teStructuredDataAssetRef[]` and a bare `ulong[]` share this descriptor and
/// differ in both width and where the value sits.
fn dyn_array_span(stu: &mut Stu, at: usize) -> Option<(usize, usize)> {
    let field_offset = stu.i32_at(at)?;
    if field_offset < 0 {
        return None;
    }
    let base = field_offset as usize;
    let dyn_block = stu.dyn_data();
    let count = le_i32(dyn_block, base)?;
    if count <= 0 || count > 4096 {
        return None;
    }
    let data_offset = le_i64(dyn_block, base + 8)?;
    if data_offset < 0 {
        return None;
    }
    let start = data_offset as usize;
    // Guard the read: a count that runs past the block is a descriptor this
    // reader does not understand, and reading it would be reading noise.
    if start.checked_add(count as usize * 16)? > dyn_block.len() {
        return None;
    }
    Some((start, count as usize))
}

/// A bare `ushort[]` in the dynamic block, two bytes per element.
fn read_u16_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u16> {
    read_prim_array(stu, instance, field_hash, 2, |b| le_u16(b, 0))
}

/// A bare `i32[]` in the dynamic block.
fn read_i32_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<i32> {
    read_prim_array(stu, instance, field_hash, 4, |b| le_i32(b, 0))
}

/// A bare `u64[]` in the dynamic block.
///
/// **No deobfuscation.** A bare `ulong[]` is not a
/// `teStructuredDataAssetRef[]`: there is no per-element field hash, and the
/// typed layer only deobfuscates what it read through `FieldType::AssetRef`
/// (`ow_stu/src/types.rs:1030`). These are the stored bytes.
fn read_u64_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u64> {
    read_prim_array(stu, instance, field_hash, 8, |b| le_u64(b, 0))
}

/// A bare `long[]` in the dynamic block.
fn read_i64_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<i64> {
    read_prim_array(stu, instance, field_hash, 8, |b| le_i64(b, 0))
}

/// Read a dynamic-block array of fixed-width elements.
///
/// The bounds check is the same as [`dyn_array_span`]'s, scaled by the element
/// width: an array whose declared count runs past the block is not read.
fn read_prim_array<T>(
    stu: &mut Stu,
    instance: usize,
    field_hash: u32,
    width: usize,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Vec<T> {
    let Some((offset, _size)) = stu.field_bytes(instance, field_hash) else {
        return Vec::new();
    };
    let at = stu.payload_start(instance) + offset;
    let Some(field_offset) = stu.i32_at(at) else {
        return Vec::new();
    };
    if field_offset < 0 {
        return Vec::new();
    }
    let dyn_block = stu.dyn_data();
    let base = field_offset as usize;
    let Some(count) = le_i32(dyn_block, base) else {
        return Vec::new();
    };
    if count <= 0 || count > 4096 {
        return Vec::new();
    }
    let Some(data_offset) = le_i64(dyn_block, base + 8) else {
        return Vec::new();
    };
    if data_offset < 0 {
        return Vec::new();
    }
    let start = data_offset as usize;
    let count = count as usize;
    match start.checked_add(count * width) {
        Some(end) if end <= dyn_block.len() => {}
        _ => return Vec::new(),
    }
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let at = start + k * width;
        let Some(bytes) = dyn_block.get(at..at + width) else {
            break;
        };
        let Some(value) = decode(bytes) else { break };
        out.push(value);
    }
    out
}

fn le_i32(data: &[u8], at: usize) -> Option<i32> {
    data.get(at..at + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn le_i64(data: &[u8], at: usize) -> Option<i64> {
    data.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn le_u16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn le_u64(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The subclass order is the reference tooling's, and the one case where it
    /// matters is a souvenir: `STU_A458D547` derives from `STUUnlock_Emote`, so
    /// a reader that tested the emote class first would call it an emote. This
    /// pins that ordering rather than trusting the reader to keep it.
    #[test]
    fn a_souvenir_is_not_an_emote() {
        assert_eq!(unlock_kind_of(class::SOUVENIR), UnlockKind::Souvenir);
        assert_eq!(unlock_kind_of(class::UNLOCK_EMOTE), UnlockKind::Emote);
        assert_ne!(
            unlock_kind_of(class::SOUVENIR),
            unlock_kind_of(class::UNLOCK_EMOTE)
        );
    }

    #[test]
    fn every_documented_subclass_maps_to_a_kind() {
        // The classes `Unlock.cs:218` lists, checked one at a time so a
        // regression names the class that broke rather than just failing.
        for (hash, kind) in [
            (class::UNLOCK_SKIN_THEME, UnlockKind::Skin),
            (class::UNLOCK_AVATAR_PORTRAIT, UnlockKind::Icon),
            (class::SOUVENIR, UnlockKind::Souvenir),
            (class::UNLOCK_EMOTE, UnlockKind::Emote),
            (class::UNLOCK_POSE, UnlockKind::VictoryPose),
            (class::UNLOCK_VOICE_LINE, UnlockKind::VoiceLine),
            (class::UNLOCK_SPRAY_PAINT, UnlockKind::Spray),
            (class::UNLOCK_CURRENCY, UnlockKind::Currency),
            (class::UNLOCK_PORTRAIT_FRAME, UnlockKind::PortraitFrame),
            (class::UNLOCK_WEAPON, UnlockKind::WeaponVariant),
            (class::UNLOCK_POTG_ANIMATION, UnlockKind::HighlightIntro),
            (
                class::UNLOCK_COMPETITIVE_CURRENCY,
                UnlockKind::CompetitiveCurrency,
            ),
            (class::UNLOCK_OWL_TOKEN, UnlockKind::OwlToken),
            (class::UNLOCK_LOOT_BOX, UnlockKind::Lootbox),
            (class::WEAPON_CHARM, UnlockKind::WeaponCharm),
            (class::HERO, UnlockKind::Hero),
            (class::BATTLE_PASS_XP, UnlockKind::BattlePassXp),
            (class::NAME_CARD, UnlockKind::NameCard),
            (class::VIRTUAL_CURRENCY, UnlockKind::VirtualCurrency),
            (class::PLAYER_TITLE, UnlockKind::PlayerTitle),
            (class::BATTLE_PASS, UnlockKind::BattlePass),
            (class::SEASON_XP_BOOST, UnlockKind::SeasonXpBoost),
            (class::BATTLE_PASS_TIER_SKIP, UnlockKind::BattlePassTierSkip),
            (class::SKIN_COMPONENT, UnlockKind::SkinComponent),
            (class::STORY_MISSION, UnlockKind::StoryMission),
            (class::WEAPON_SKIN, UnlockKind::WeaponSkin),
            (
                class::COMPETITIVE_SIGNATURE,
                UnlockKind::CompetitiveSignature,
            ),
            (class::UNMAPPED_1EB22BDB, UnlockKind::Unmapped),
        ] {
            assert_eq!(unlock_kind_of(hash), kind, "class 0x{hash:08X}");
        }
    }

    #[test]
    fn the_intermediate_classes_are_unknown_too() {
        // The reference tooling's switch does not list the two intermediates,
        // and neither does this reader: an asset rooted at one is a real shipped
        // shape with no type name, not a misread.
        for hash in [
            class::UNLOCK,
            class::UNLOCK_DISPLAY,
            class::UNLOCK_PLAIN,
            0xDEAD_BEEF,
        ] {
            assert_eq!(unlock_kind_of(hash), UnlockKind::Unknown);
        }
    }

    #[test]
    fn the_traditional_list_matches_the_reference() {
        // `Unlock.cs:135` lists thirteen kinds as "traditional". A cosmetic
        // outside that list is a reading bug, not a new unlock type.
        let traditional: Vec<UnlockKind> = [
            UnlockKind::Icon,
            UnlockKind::Spray,
            UnlockKind::Skin,
            UnlockKind::HighlightIntro,
            UnlockKind::VictoryPose,
            UnlockKind::VoiceLine,
            UnlockKind::Emote,
            UnlockKind::Souvenir,
            UnlockKind::NameCard,
            UnlockKind::PlayerTitle,
            UnlockKind::WeaponCharm,
            UnlockKind::WeaponVariant,
            UnlockKind::WeaponSkin,
        ]
        .into_iter()
        .collect();
        for kind in [
            UnlockKind::Icon,
            UnlockKind::Spray,
            UnlockKind::Skin,
            UnlockKind::HighlightIntro,
            UnlockKind::VictoryPose,
            UnlockKind::VoiceLine,
            UnlockKind::Emote,
            UnlockKind::Souvenir,
            UnlockKind::NameCard,
            UnlockKind::PlayerTitle,
            UnlockKind::WeaponCharm,
            UnlockKind::WeaponVariant,
            UnlockKind::WeaponSkin,
        ] {
            assert!(kind.is_traditional(), "{kind} should be traditional");
        }
        for kind in [
            UnlockKind::Currency,
            UnlockKind::BattlePass,
            UnlockKind::Lootbox,
            UnlockKind::Hero,
            UnlockKind::Unknown,
        ] {
            assert!(!kind.is_traditional(), "{kind} should not be");
        }
        assert_eq!(traditional.len(), 13);
    }

    #[test]
    fn rarity_and_category_carry_unrecognised_values() {
        assert_eq!(Rarity::from_raw(3), Rarity::Legendary);
        assert_eq!(Rarity::from_raw(7), Rarity::Unnamed7);
        assert_eq!(Rarity::from_raw(-1), Rarity::None);
        assert_eq!(Rarity::from_raw(99), Rarity::Unknown(99));

        assert_eq!(Category::from_raw(0), Category::Skin);
        assert_eq!(Category::from_raw(0x08), Category::VoiceLine);
        assert_eq!(Category::from_raw(-1), Category::Unknown(-1));
    }

    #[test]
    fn a_price_is_never_invented() {
        // `Unlock::price` returns `None` unconditionally. If someone ever fills
        // it in, this test is the reminder that the value has to come from a
        // field, not from the product id.
        let unlock = Unlock {
            asset: 0,
            root_class: class::UNLOCK_SKIN_THEME,
            kind: UnlockKind::Skin,
            name: None,
            description: None,
            available_in: None,
            rarity: Some(Rarity::Legendary),
            category: Some(Category::Skin),
            categories: Vec::new(),
            product_id: Some(1234),
            resource_key: None,
            esports_team: None,
            loot_box_kind: None,
            portrait_rank: None,
            level: None,
            stars: None,
            grants: Vec::new(),
            values: Vec::new(),
        };
        assert_eq!(unlock.price(), None);
    }

    #[test]
    fn amount_skips_the_fields_that_are_not_amounts() {
        let mut unlock = Unlock {
            asset: 0,
            root_class: class::UNLOCK_PORTRAIT_FRAME,
            kind: UnlockKind::PortraitFrame,
            name: None,
            description: None,
            available_in: None,
            rarity: None,
            category: None,
            categories: Vec::new(),
            product_id: None,
            resource_key: None,
            esports_team: None,
            loot_box_kind: None,
            portrait_rank: Some(PortraitFrameRank::Gold),
            level: Some(3),
            stars: Some(1),
            grants: Vec::new(),
            // The order matters: rank and level come first, as the reader
            // writes them, and `amount` must skip both.
            values: vec![("m_rank", 3), ("m_level", 3), ("m_stars", 1)],
        };
        assert_eq!(unlock.amount(), None);
        unlock.values.push(("m_currency", 2500));
        assert_eq!(unlock.amount(), Some(2500));
        assert_eq!(unlock.amount_field(), Some("m_currency"));
    }
}

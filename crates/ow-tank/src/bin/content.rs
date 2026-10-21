//! Read the content surface: unlocks, cosmetics, loot boxes, achievements,
//! seasons, conversations.
//!
//! Two jobs. `--types` is a census — how many assets of each content type this
//! install ships, how many opened, and which class each one's root is — which
//! is how the shapes were found in the first place. A subcommand then walks one
//! surface and prints a one-line summary per record.
//!
//! ```text
//! ow-tank-content --types                       # every content type, counted
//! ow-tank-content unlocks --limit 20            # 20 unlock records
//! ow-tank-content skinthemes --limit 5
//! ow-tank-content lootboxes                    # box presentation
//! ow-tank-content rules                        # the loot-box rarity tables
//! ow-tank-content --guid 0x0a5000000000012     # one asset of any content type
//! ```
//!
//! `--limit` bounds how many *records* are printed. It never bounds the census:
//! `--types` counts every asset of every type whatever `--limit` says.
//!
//! Read-only: opens the install the same way the rest of the tree does, and
//! writes nothing.

use ow_tank::unlocks::{self, types, Survey};
use ow_tank::{guid, GameData, ManifestSelector};

/// Subcommand name to asset type.
///
/// [`types::by_name`] matches the type label with spaces removed, which covers
/// `skintheme`, `lootbox` and `stat`. The table below is for the names the
/// usage text advertises, which are the plurals and the shortened forms. `rules`
/// has no asset type of its own — the loot-box rarity table lives in a settings
/// asset — so it is not here.
fn surface(name: &str) -> Option<u16> {
    const ALIASES: &[(&str, u16)] = &[
        ("unlocks", types::UNLOCK),
        ("skinthemes", types::SKIN_THEME),
        ("skins", types::SKIN_THEME),
        ("lootboxes", types::LOOT_BOX),
        ("achievements", types::ACHIEVEMENT),
        ("seasons", types::RANKED_SEASON),
        ("rankedseasons", types::RANKED_SEASON),
        ("progressions", types::PROGRESSION_UNLOCKS),
        ("conversations", types::VOICE_CONVERSATION),
        ("voiceconversations", types::VOICE_CONVERSATION),
        ("stats", types::STAT),
        ("catalogs", types::CATALOG),
        ("customizableskinthemes", types::CUSTOMIZABLE_SKIN_THEME),
    ];
    let squashed: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == squashed)
        .map(|(_, t)| *t)
        .or_else(|| types::by_name(&squashed))
}

fn parse_limit(args: &[String]) -> usize {
    args.iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}

/// How many assets of each type `--types` may open. The reference install
/// records 19,122 unlocks and downloads none, so an uncapped census spends
/// minutes on CASC misses; this bounds it and the census says it was bounded.
const DEFAULT_SCAN_CAP: usize = 400;

fn parse_scan_cap(args: &[String]) -> usize {
    args.iter()
        .position(|a| a == "--scan")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_SCAN_CAP)
}

fn parse_guid(args: &[String]) -> Option<u64> {
    args.iter()
        .position(|a| a == "--guid" || a == "--from")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
}

fn print_usage() {
    eprintln!(
        "usage: ow-tank-content <surface> [--limit N] | --types | --guid 0xGUID | rules\n\
         \n\
         surfaces: unlocks, skinthemes, lootboxes, achievements, seasons,\n\
         \x20         progressions, conversations, stats, catalogs\n\
         \x20         (or a literal type id such as 0x0a5)\n\
         flags:    --types   census every content type\n\
         \x20         --limit N records printed per surface (default 10)\n\
         \x20         --scan N  assets opened per type (default {DEFAULT_SCAN_CAP})\n\
         \x20         --guid G  dump one asset, whatever its type"
    );
}

fn print_census(survey: &Survey) {
    println!(
        "type 0x{:03X} {} ({}): {} asset(s) registered, {} scanned, {} readable, {} parsed, \
         {} not downloaded, {} unparsed, {} version-1, {} instance(s)",
        survey.type_id,
        types::label(survey.type_id),
        survey.type_id,
        survey.assets,
        survey.scanned,
        survey.readable,
        survey.parsed(),
        survey.unreadable,
        survey.unparsed,
        survey.version_1,
        survey.instances,
    );
    if survey.root_classes.is_empty() {
        // Three situations land here and they are worth telling apart: the type
        // is not in the registry, every asset is recorded but none downloaded,
        // or everything opened and nothing parsed.
        if survey.assets == 0 {
            println!("  this type is not in the loaded manifests at all");
        } else if survey.readable == 0 {
            let assets = survey.assets;
            println!("  all {assets} asset(s) are recorded but not downloaded; nothing to read");
        } else {
            let readable = survey.readable;
            println!("  {readable} asset(s) opened but none parsed; see the errors below");
        }
        for line in &survey.errors {
            println!("    parse error: {line}");
        }
        return;
    }
    println!("  root instance classes:");
    let mut rows: Vec<(&u32, &usize)> = survey.root_classes.iter().collect();
    // Most common first, then by hash, so the same install always prints the
    // same table.
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (hash, count) in rows {
        let kind = match *hash {
            unlocks::class::UNLOCK => "STUUnlock",
            unlocks::class::UNLOCK_DISPLAY => "STU_3021DDED",
            unlocks::class::UNLOCK_PLAIN => "STU_895A2A7A",
            unlocks::class::UNLOCKS => "STUUnlocks",
            unlocks::class::UNLOCK_SKIN_THEME => "STUUnlock_SkinTheme",
            unlocks::class::UNLOCK_AVATAR_PORTRAIT => "STUUnlock_AvatarPortrait",
            unlocks::class::SOUVENIR => "STU_A458D547 (souvenir)",
            unlocks::class::UNLOCK_EMOTE => "STUUnlock_Emote",
            unlocks::class::UNLOCK_POSE => "STUUnlock_Pose",
            unlocks::class::UNLOCK_VOICE_LINE => "STUUnlock_VoiceLine",
            unlocks::class::UNLOCK_SPRAY_PAINT => "STUUnlock_SprayPaint",
            unlocks::class::UNLOCK_CURRENCY => "STUUnlock_Currency",
            unlocks::class::UNLOCK_PORTRAIT_FRAME => "STUUnlock_PortraitFrame",
            unlocks::class::UNLOCK_WEAPON => "STUUnlock_Weapon",
            unlocks::class::UNLOCK_POTG_ANIMATION => "STUUnlock_POTGAnimation",
            unlocks::class::UNLOCK_COMPETITIVE_CURRENCY => "STUUnlock_CompetitiveCurrency",
            unlocks::class::UNLOCK_OWL_TOKEN => "STUUnlock_OWLToken",
            unlocks::class::UNLOCK_LOOT_BOX => "STUUnlock_LootBox",
            unlocks::class::WEAPON_CHARM => "STU_6A808718 (weapon charm)",
            unlocks::class::HERO => "STU_C3C6FD9E (hero)",
            unlocks::class::BATTLE_PASS_XP => "STU_7A1A4764 (battle pass XP)",
            unlocks::class::NAME_CARD => "STU_DB1B05B5 (name card)",
            unlocks::class::VIRTUAL_CURRENCY => "STU_514C0F6B (virtual currency)",
            unlocks::class::PLAYER_TITLE => "STU_52AB57E9 (player title)",
            unlocks::class::BATTLE_PASS => "STU_AD84E2AA (battle pass)",
            unlocks::class::SEASON_XP_BOOST => "STU_184D5944 (season XP boost)",
            unlocks::class::BATTLE_PASS_TIER_SKIP => "STU_80C1169E (tier skip)",
            unlocks::class::SKIN_COMPONENT => "STU_3F17D547 (skin component)",
            unlocks::class::STORY_MISSION => "STU_A85D31BF (story mission)",
            unlocks::class::WEAPON_SKIN => "STU_2448F3AA (weapon skin)",
            unlocks::class::COMPETITIVE_SIGNATURE => "STU_FA317B7D (signature)",
            unlocks::class::UNMAPPED_1EB22BDB => "STU_1EB22BDB (unmapped)",
            unlocks::class::SKIN_THEME => "STUSkinTheme",
            unlocks::class::MYTHIC_SKIN => "STU_EF85B312 (mythic skin)",
            unlocks::class::LOOT_BOX => "STULootBox",
            unlocks::class::ACHIEVEMENT => "STUAchievement",
            unlocks::class::RANKED_SEASON => "STURankedSeason",
            unlocks::class::PROGRESSION_UNLOCKS => "STUProgressionUnlocks",
            unlocks::class::VOICE_CONVERSATION => "STUVoiceConversation",
            unlocks::class::STAT => "STUStat",
            unlocks::class::CATALOG => "STUCatalog",
            _ => "unrecognised",
        };
        println!("    0x{hash:08X}  {kind:<38} {count} asset(s)");
    }
    if survey.unrecognised > 0 {
        let unrecognised = survey.unrecognised;
        println!("  {unrecognised} asset(s) have a root class no reader here claims");
    }
    for line in &survey.errors {
        println!("  parse error: {line}");
    }
    if survey.capped {
        let scanned = survey.scanned;
        let assets = survey.assets;
        println!(
            "  CAPPED: {scanned} of {assets} asset(s) were opened, striding over the rest \
             (`--scan N` raises this)"
        );
    }
}

/// Print a bounded sample of one surface.
fn print_records(data: &GameData, type_id: u16, limit: usize, cap: usize) {
    let survey = unlocks::survey_type_capped(data, type_id, limit, cap);
    print_census(&survey);
    if survey.records.is_empty() {
        return;
    }
    println!("  first {} record(s):", survey.records.len());
    for record in &survey.records {
        println!("    0x{:016x}  {}", record.asset(), record.summary());
    }
    if survey.records.len() < survey.parsed() {
        println!(
            "  ({}/{} parsed record(s) printed; raise --limit to see more)",
            survey.records.len(),
            survey.parsed()
        );
    }
}

fn print_one(data: &GameData, asset: u64) -> std::process::ExitCode {
    let type_id = guid::type_of(asset);
    println!(
        "0x{asset:016x}  type 0x{:03X} ({})  index {}",
        type_id,
        types::label(type_id),
        guid::index_of(asset),
    );
    if !types::ALL.contains(&type_id) {
        eprintln!("note: 0x{type_id:03X} is not one of the content types this tool surveys");
        return std::process::ExitCode::from(1);
    }
    let survey = unlocks::survey_type(data, type_id, 1);
    // `survey_type` walks the whole type, so find this asset's own record by
    // re-reading it directly rather than reporting the type's census.
    drop(survey);
    let mut stu = match data.stu_of(asset) {
        Ok(stu) => stu,
        Err(e) => {
            eprintln!("0x{asset:016x}: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    println!(
        "  {} instance(s), {}",
        stu.instances.len(),
        if stu.is_v1() {
            "version 1 (the raw field reader has no per-instance field bag here)"
        } else {
            "version 2"
        }
    );
    for i in 0..stu.instances.len().min(32) {
        let hash = stu.instances[i].hash;
        let fields = stu.fields(i).len();
        println!(
            "    [{i:>3}] class 0x{hash:08X}  payload {} B  {fields} field(s)",
            stu.data(i).len()
        );
    }
    let record =
        match type_id {
            t if t == types::UNLOCK => unlocks::read_unlock_in(&mut stu, asset)
                .map(|r| unlocks::Record::Unlock(Box::new(r))),
            t if t == types::SKIN_THEME || t == types::CUSTOMIZABLE_SKIN_THEME => {
                unlocks::read_skin_theme_in(&mut stu, asset)
                    .map(|r| unlocks::Record::SkinTheme(Box::new(r)))
            }
            t if t == types::LOOT_BOX => unlocks::read_loot_box_in(&mut stu, asset)
                .map(|r| unlocks::Record::LootBox(Box::new(r))),
            t if t == types::ACHIEVEMENT => unlocks::read_achievement_in(&mut stu, asset)
                .map(|r| unlocks::Record::Achievement(Box::new(r))),
            t if t == types::RANKED_SEASON => unlocks::read_ranked_season_in(&mut stu, asset)
                .map(|r| unlocks::Record::RankedSeason(Box::new(r))),
            t if t == types::PROGRESSION_UNLOCKS => unlocks::read_progression_in(&mut stu, asset)
                .map(|r| unlocks::Record::ProgressionUnlocks(Box::new(r))),
            t if t == types::VOICE_CONVERSATION => unlocks::read_conversation_in(&mut stu, asset)
                .map(|r| unlocks::Record::VoiceConversation(Box::new(r))),
            t if t == types::STAT => {
                unlocks::read_stat_in(&mut stu, asset).map(|r| unlocks::Record::Stat(Box::new(r)))
            }
            t if t == types::CATALOG => unlocks::read_catalog_in(&mut stu, asset)
                .map(|r| unlocks::Record::Catalog(Box::new(r))),
            _ => None,
        };
    match record {
        Some(record) => println!("  {}", record.summary()),
        None => {
            println!("  no reader claims this asset's root class");
            return std::process::ExitCode::from(1);
        }
    }
    std::process::ExitCode::SUCCESS
}

/// The loot-box rarity tables, which live in a settings asset rather than in a
/// loot-box asset.
fn print_rules(data: &GameData) {
    let rules = unlocks::loot_box_rules(data);
    if rules.is_empty() {
        println!("no STULootBoxRules found in any generic-settings asset (type 0x054)");
        return;
    }
    println!("{} loot-box rarity table(s):", rules.len());
    for rule in &rules {
        println!(
            "  from settings asset 0x{:016x}, theme {}",
            rule.asset,
            rule.kind.map(|k| k.to_string()).unwrap_or("unset".into()),
        );
        println!(
            "    unnamed m_24391F9B={:?} m_53B8C818={:?}",
            rule.unnamed_24391f9b, rule.unnamed_53b8c818
        );
        if rule.rarity_rules.is_empty() && rule.currency_rarity_rules.is_empty() {
            println!("    no rarity rules on this table");
        }
        for (label, rules) in [
            ("rarity", &rule.rarity_rules),
            ("currency", &rule.currency_rarity_rules),
        ] {
            for entry in rules.iter() {
                println!(
                    "    {label:<8} rarity {:<10} m_A07D6F99={} m_CBE06580={}",
                    entry.rarity.to_string(),
                    entry.weight_a,
                    entry.weight_b
                );
            }
        }
    }
    println!(
        "note: what m_A07D6F99 and m_CBE06580 weigh is not established; they are \\
         printed as stored."
    );
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return std::process::ExitCode::from(2);
    }

    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    if let Some(asset) = parse_guid(&args) {
        return print_one(&data, asset);
    }

    let subcommand = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_default();

    if args.iter().any(|a| a == "--types") || subcommand == "types" {
        let limit = parse_limit(&args);
        let cap = parse_scan_cap(&args);
        for survey in unlocks::survey_content_capped(&data, limit, cap) {
            print_census(&survey);
        }
        println!("\nmanifests loaded: {}", data.manifests.join(", "));
        return std::process::ExitCode::SUCCESS;
    }

    if subcommand == "rules" {
        print_rules(&data);
        return std::process::ExitCode::SUCCESS;
    }

    if subcommand.is_empty() {
        print_usage();
        return std::process::ExitCode::from(2);
    }

    let Some(type_id) = surface(&subcommand) else {
        eprintln!("unknown surface {subcommand:?}");
        print_usage();
        return std::process::ExitCode::from(2);
    };
    print_records(&data, type_id, parse_limit(&args), parse_scan_cap(&args));
    std::process::ExitCode::SUCCESS
}

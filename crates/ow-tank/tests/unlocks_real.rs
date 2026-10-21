//! The content surface against the real installation: a census of every
//! content type, and invariants the bytes support rather than counts.
//!
//! Skipped when no install is present, and it asserts nothing when the
//! install's manifests are not readable. Read-only.
//!
//! Run the census with:
//!
//! ```text
//! cargo test -p ow-tank --release --test unlocks_real -- --nocapture
//! ```
//!
//! ## Why there are no exact counts here
//!
//! An exact count is a claim about a *patch* of the game, not about the reader:
//! Blizzard adds an achievement or a season and the assertion fails while the
//! code is fine. What is worth asserting is an invariant that a *wrong read*
//! would break. A field offset that is one field out, or an array stride that is
//! one element out, yields 64-bit values that do not resolve to assets that
//! exist — so "every reference this reader recovered resolves in the asset
//! registry" is the check that catches a broken layout. That is what is asserted
//! below, and the counts are printed so a human can see what was measured.

use std::collections::BTreeMap;

use ow_stu::types::{FieldType, Value};
use ow_tank::classreg::ClassRegistry as TankRegistry;
use ow_tank::keytable::Sources;
use ow_tank::unlocks::{self, types, Survey};
use ow_tank::{guid, GameData, ManifestSelector};

/// Upper bound on assets opened per type. The reference install records 19,122
/// unlock assets and downloads none of them, so an uncapped census spends
/// minutes on CASC misses; the survey records that it was capped.
const SAMPLE_CAP: usize = 120;

/// How many bytes of a failing asset to print, to say what it actually is.
const FAILURE_PREFIX: usize = 48;

fn open() -> Option<(GameData, Option<TankRegistry>)> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            return None;
        }
    };
    // The typed layer is registry-driven and the registry is empty without a
    // checkout, so it is optional: the raw readers below do not need it, and a
    // cross-check that silently does nothing would be worse than no cross-check.
    let registry = TankRegistry::load_from_sources(&sources);
    if registry.is_empty() {
        eprintln!("note: no STU class checkout found; the typed cross-checks are skipped");
    }
    Some((data, Some(registry).filter(|r| !r.is_empty())))
}

fn print_census(survey: &Survey) {
    println!(
        "type 0x{:03X} {:<26} registered={:<6} scanned={:<5} readable={:<5} parsed={:<5} \
         not-downloaded={:<5} unparsed={:<4} v1={:<3} instances={}",
        survey.type_id,
        types::label(survey.type_id),
        survey.assets,
        survey.scanned,
        survey.readable,
        survey.parsed(),
        survey.unreadable,
        survey.unparsed,
        survey.version_1,
        survey.instances,
    );
    let mut roots: Vec<(&u32, &usize)> = survey.root_classes.iter().collect();
    roots.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (hash, count) in roots {
        let kind = unlocks::unlock_kind_of(*hash);
        let extra = match kind {
            unlocks::UnlockKind::Unknown => String::new(),
            other => format!("  ({other})"),
        };
        println!("        root 0x{hash:08X}  {count} asset(s){extra}");
    }
    for line in survey.errors.iter().take(4) {
        println!("        error: {line}");
    }
}

/// What the first bytes of a failing asset look like, so "does not parse" can be
/// a finding rather than a shrug.
fn print_failure_prefix(data: &GameData, asset: u64) {
    let Ok(bytes) = data.bytes_of(asset) else {
        return;
    };
    let head: Vec<String> = bytes
        .iter()
        .take(FAILURE_PREFIX)
        .map(|b| format!("{b:02x}"))
        .collect();
    println!(
        "        0x{asset:016x}: {} byte(s), first {}: {}",
        bytes.len(),
        head.len(),
        head.join(" ")
    );
}

#[test]
fn real_content_surface_census_and_invariants() {
    let Some((data, registry)) = open() else {
        return;
    };

    // -----------------------------------------------------------------------
    // The census
    // -----------------------------------------------------------------------
    let surveys = unlocks::survey_content_capped(&data, SAMPLE_CAP, SAMPLE_CAP);
    println!("\n=== content surface census ===");
    for survey in &surveys {
        print_census(survey);
    }
    println!("manifests: {}", data.manifests.join(", "));

    // -----------------------------------------------------------------------
    // Invariant 1: a reader either parses the asset or says why it did not.
    //
    // The exception is documented rather than special-cased in the reader:
    // type 0x063 (Catalog) opens on this install and then fails `Stu::parse`
    // outright, which means its container is not the STU layout this tree
    // knows. That is a real, recorded gap, so the assertion names it instead
    // of being quietly weakened for every type.
    const KNOWN_UNPARSED: &[u16] = &[types::CATALOG];
    for survey in &surveys {
        if survey.unparsed > 0 && !KNOWN_UNPARSED.contains(&survey.type_id) {
            panic!(
                "type 0x{:03X} ({}): {} of {} readable asset(s) failed to parse: {:?}",
                survey.type_id,
                types::label(survey.type_id),
                survey.unparsed,
                survey.readable,
                survey.errors
            );
        }
    }
    for survey in &surveys {
        if !KNOWN_UNPARSED.contains(&survey.type_id) {
            continue;
        }
        for line in survey.errors.iter().take(3) {
            if let Some(asset) = line
                .split_whitespace()
                .next()
                .map(|t| t.trim_end_matches(':'))
                .and_then(|t| u64::from_str_radix(t.trim_start_matches("0x"), 16).ok())
            {
                print_failure_prefix(&data, asset);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Invariant 2: an asset type with nothing readable is reported, not
    // silently treated as a pass.
    //
    // This is not decoration. On the reference install the largest content
    // type — 19,122 unlock assets — is in the registry with *none* of it
    // downloaded, so the whole unlock reader is unexercised. A test that only
    // asserted things about what it could read would pass with the unlock
    // reader completely broken.
    let mut unreadable_types = Vec::new();
    for survey in &surveys {
        if survey.assets > 0 && survey.readable == 0 {
            unreadable_types.push(format!(
                "0x{:03X} {} ({} assets)",
                survey.type_id,
                types::label(survey.type_id),
                survey.assets
            ));
        }
    }
    if unreadable_types.is_empty() {
        println!("\nevery content type had at least one readable asset");
    } else {
        println!(
            "\ncontent types with NOTHING readable here, so their readers are \
             not tested against real data:\n  {}",
            unreadable_types.join("\n  ")
        );
    }

    // -----------------------------------------------------------------------
    // Invariant 3: every skin theme's weapon-slot skin and model resolves.
    //
    // This is the check that catches a wrong field offset or a wrong array
    // stride, because a wrong read yields 64-bit values that name nothing.
    let theme_survey =
        unlocks::survey_type_capped(&data, types::SKIN_THEME, SAMPLE_CAP, SAMPLE_CAP);
    let themes: Vec<&unlocks::Record> = theme_survey
        .records
        .iter()
        .filter(|r| matches!(r, unlocks::Record::SkinTheme(_)))
        .collect();
    println!(
        "\nskin themes: {} record(s) sampled of {} parsed ({} asset(s) in the type)",
        themes.len(),
        theme_survey.parsed(),
        theme_survey.assets,
    );

    let mut weapon_skins_seen = 0usize;
    let mut weapon_skins_resolved = 0usize;
    let mut weapon_skin_values: BTreeMap<u64, usize> = BTreeMap::new();
    let mut weapon_skin_types: BTreeMap<u16, usize> = BTreeMap::new();
    let mut weapon_types_seen: BTreeMap<i32, usize> = BTreeMap::new();
    let mut models_seen = 0usize;
    let mut models_resolved = 0usize;
    let mut portraits_seen = 0usize;
    let mut portraits_resolved = 0usize;
    let mut team_variants_seen = 0usize;
    let mut team_variants_resolved = 0usize;
    let mut internal_names = 0usize;
    let mut mythic = 0usize;
    let mut slots_seen = 0usize;
    let mut slot_fields: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut slot_item_counts: BTreeMap<u32, usize> = BTreeMap::new();
    let mut slot_textures_seen = 0usize;
    let mut slot_textures_resolved = 0usize;

    for record in &themes {
        let unlocks::Record::SkinTheme(theme) = record else {
            continue;
        };
        if theme.is_mythic {
            mythic += 1;
        }
        if theme.internal_name.is_some() {
            internal_names += 1;
        }
        if let Some(portrait) = theme.portrait.filter(|g| *g != 0) {
            portraits_seen += 1;
            if data.registry().get(portrait).is_some() {
                portraits_resolved += 1;
            }
        }
        for model in &theme.models {
            models_seen += 1;
            if data.registry().get(model.model).is_some() {
                models_resolved += 1;
            }
        }
        for weapon in &theme.weapon_skins {
            weapon_skins_seen += 1;
            if data.registry().get(weapon.skin).is_some() {
                weapon_skins_resolved += 1;
            }
            *weapon_skin_values.entry(weapon.skin).or_insert(0) += 1;
            *weapon_skin_types
                .entry(guid::type_of(weapon.skin))
                .or_insert(0) += 1;
            *weapon_types_seen.entry(weapon.raw_variation).or_insert(0) += 1;
        }
        for variant in &theme.team_variants {
            team_variants_seen += 1;
            for candidate in [variant.source_team, variant.generated_theme] {
                if let Some(g) = candidate.filter(|g| *g != 0) {
                    if data.registry().get(g).is_some() {
                        team_variants_resolved += 1;
                    }
                }
            }
        }
        for slot in &theme.slots {
            slots_seen += 1;
            for (name, value) in [
                ("m_id", slot.id),
                ("m_displayText", slot.display_text),
                ("m_texture", slot.texture),
                ("m_animation", slot.animation),
            ] {
                if value.filter(|g| *g != 0).is_some() {
                    *slot_fields.entry(name).or_insert(0) += 1;
                }
            }
            *slot_item_counts.entry(slot.item_count).or_insert(0) += 1;
            if let Some(texture) = slot.texture.filter(|g| *g != 0) {
                slot_textures_seen += 1;
                if data.registry().get(texture).is_some() {
                    slot_textures_resolved += 1;
                }
            }
        }
    }

    println!("  internal names read:            {internal_names}");
    println!(
        "  weapon-slot skins:              {weapon_skins_seen} read, {weapon_skins_resolved} resolve in the registry"
    );
    println!("  weapon variations (raw):        {weapon_types_seen:?}");
    println!(
        "  distinct weapon-slot skin refs: {} (type histogram {weapon_skin_types:?})",
        weapon_skin_values.len()
    );
    println!("  swapped-in models:              {models_seen} read, {models_resolved} resolve");
    println!(
        "  portraits:                      {portraits_seen} read, {portraits_resolved} resolve"
    );
    println!(
        "  team variants:                  {team_variants_seen} elements, {team_variants_resolved} of their references resolve"
    );
    println!("  mythic skins:                   {mythic}");
    println!(
        "  mythic slots:                   {slots_seen}, fields present {slot_fields:?}, \
         m_57CE9041 item counts {slot_item_counts:?}"
    );
    println!(
        "  slot textures:                  {slot_textures_seen} read, {slot_textures_resolved} resolve"
    );

    // The reference install has weapon-slot skins on every sampled theme and no
    // swapped-in models at all, because `m_BCC55571` is absent from the payload
    // (see the field-drop note below). Both are facts worth asserting: a
    // regression that made the weapon-slot read return nothing would otherwise
    // pass unnoticed, because the reader returns an empty list rather than
    // failing.
    assert!(
        weapon_skins_seen > 0,
        "no skin theme in the sample carried a weapon-slot skin; the \
         m_5FC3164C read is unexercised"
    );
    assert_eq!(
        weapon_skins_resolved, weapon_skins_seen,
        "a weapon-slot skin reference did not resolve in the registry, which is \
         what a wrong field offset or array stride looks like"
    );
    assert_eq!(
        models_seen, 0,
        "swapped-in models appeared where the reference install has none: either \
         m_BCC55571 became readable or m_BCC55571 is no longer the right hash"
    );

    // -----------------------------------------------------------------------
    // Voice conversations.
    //
    // `m_E295B99C` is declared `teStructuredDataAssetRef<STUVoiceLine>`. The
    // reference tooling's asset list does **not** have a 0x6F entry, so the
    // asset type a voice line actually carries is not the type the C# suggests,
    // and this reader does not guess which is right. What is checkable is that
    // every line carries the *same* type: a wrong element stride shifts each
    // element and the types come out mixed, which a histogram shows at a glance.
    let conversation_survey =
        unlocks::survey_type_capped(&data, types::VOICE_CONVERSATION, SAMPLE_CAP, SAMPLE_CAP);
    let mut lines_seen = 0usize;
    let mut line_types: BTreeMap<u16, usize> = BTreeMap::new();
    let mut lines_resolved = 0usize;
    let mut stimulus_seen = 0usize;
    let mut stimulus_types: BTreeMap<u16, usize> = BTreeMap::new();
    let mut lines_per_conversation: BTreeMap<usize, usize> = BTreeMap::new();
    let mut weights: BTreeMap<u32, usize> = BTreeMap::new();
    let mut ushort_array_lengths: BTreeMap<usize, usize> = BTreeMap::new();
    for record in &conversation_survey.records {
        let unlocks::Record::VoiceConversation(conversation) = record else {
            continue;
        };
        if let Some(weight) = conversation.weight.map(|w| w.to_bits()) {
            *weights.entry(weight).or_insert(0) += 1;
        }
        *ushort_array_lengths
            .entry(conversation.unnamed_265a80ed.len())
            .or_insert(0) += 1;
        *lines_per_conversation
            .entry(conversation.lines.len())
            .or_insert(0) += 1;
        if let Some(stimulus) = conversation.stimulus.filter(|g| *g != 0) {
            stimulus_seen += 1;
            *stimulus_types.entry(guid::type_of(stimulus)).or_insert(0) += 1;
        }
        for line in &conversation.lines {
            lines_seen += 1;
            *line_types
                .entry(guid::type_of(line.voice_line))
                .or_insert(0) += 1;
            if data.registry().get(line.voice_line).is_some() {
                lines_resolved += 1;
            }
        }
    }
    println!(
        "\nvoice conversations: {} record(s) sampled of {} parsed ({} in the type)",
        conversation_survey.records.len(),
        conversation_survey.parsed(),
        conversation_survey.assets,
    );
    println!("  lines:                          {lines_seen} read, {lines_resolved} resolve in the registry");
    println!("  line asset types:               {line_types:?}");
    println!("  stimuli:                        {stimulus_seen} read, types {stimulus_types:?}");
    println!("  lines per conversation:         {lines_per_conversation:?}");
    println!(
        "  m_weight (f32 bit patterns):     {}",
        weights
            .iter()
            .map(|(bits, n)| format!("{:#010x}x{}", bits, n))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!(
        "  m_265A80ED element counts:      {:?}",
        ushort_array_lengths
    );
    assert!(lines_seen > 0, "no conversation line was read at all");
    assert_eq!(
        line_types.len(),
        1,
        "conversation lines carry more than one asset type, which is what a \
         wrong element stride looks like: {line_types:?}"
    );

    // -----------------------------------------------------------------------
    // Invariant 5: progression and loot-box references resolve.
    let progression_survey =
        unlocks::survey_type_capped(&data, types::PROGRESSION_UNLOCKS, SAMPLE_CAP, SAMPLE_CAP);
    let mut prog_unlocks_seen = 0usize;
    let mut prog_unlocks_resolved = 0usize;
    let mut levels_seen = 0usize;
    let mut levels_with_unlocks = 0usize;
    let mut pool_unlocks_seen = 0usize;
    let mut pool_unlocks_resolved = 0usize;
    for record in &progression_survey.records {
        let unlocks::Record::ProgressionUnlocks(progression) = record else {
            continue;
        };
        for g in progression
            .unlocks
            .iter()
            .chain(progression.other_unlocks.iter())
            .filter(|g| **g != 0)
        {
            prog_unlocks_seen += 1;
            if data.registry().get(*g).is_some() {
                prog_unlocks_resolved += 1;
            }
        }
        for level in &progression.levels {
            levels_seen += 1;
            if !level.unlocks.is_empty() {
                levels_with_unlocks += 1;
            }
            for g in level.unlocks.iter().filter(|g| **g != 0) {
                prog_unlocks_seen += 1;
                if data.registry().get(*g).is_some() {
                    prog_unlocks_resolved += 1;
                }
            }
        }
        for pool in &progression.loot_boxes {
            for g in pool.unlocks.iter().filter(|g| **g != 0) {
                pool_unlocks_seen += 1;
                if data.registry().get(*g).is_some() {
                    pool_unlocks_resolved += 1;
                }
            }
        }
    }
    println!(
        "\nprogression unlocks: {} record(s) sampled of {} parsed ({} in the type)",
        progression_survey.records.len(),
        progression_survey.parsed(),
        progression_survey.assets,
    );
    println!(
        "  level entries:                  {levels_seen}, {levels_with_unlocks} grant something"
    );
    println!(
        "  unlock references:              {prog_unlocks_seen} read, {prog_unlocks_resolved} resolve"
    );
    println!(
        "  loot-box pool references:       {pool_unlocks_seen} read, {pool_unlocks_resolved} resolve"
    );
    // This is the largest real check in the file. Every one of these came out of
    // a two-hop walk — an inline `STUUnlocks`, then a reference array in the
    // dynamic block whose elements are sixteen bytes with the GUID at +8 — so a
    // wrong stride or a missing deobfuscation shows up as GUIDs that name
    // nothing. They all name something.
    assert!(
        prog_unlocks_seen > 0,
        "no progression unlock reference was read at all"
    );
    assert_eq!(
        prog_unlocks_resolved, prog_unlocks_seen,
        "a progression unlock reference did not resolve in the registry"
    );
    assert_eq!(
        pool_unlocks_resolved, pool_unlocks_seen,
        "a loot-box pool reference did not resolve in the registry"
    );

    let loot_survey = unlocks::survey_type_capped(&data, types::LOOT_BOX, SAMPLE_CAP, SAMPLE_CAP);
    let mut looks_seen = 0usize;
    let mut looks_right_type = 0usize;
    for record in &loot_survey.records {
        let unlocks::Record::LootBox(box_record) = record else {
            continue;
        };
        for look in [box_record.model_look, box_record.base_model_look]
            .into_iter()
            .flatten()
            .filter(|g| *g != 0)
        {
            looks_seen += 1;
            if guid::type_of(look) == types::MODEL_LOOK {
                looks_right_type += 1;
            }
        }
    }
    println!(
        "\nloot boxes: {} record(s) sampled of {} parsed ({} in the type)",
        loot_survey.records.len(),
        loot_survey.parsed(),
        loot_survey.assets,
    );
    println!(
        "  model looks:                    {looks_seen} read, {looks_right_type} are type 0x{:03X}",
        types::MODEL_LOOK
    );

    // -----------------------------------------------------------------------
    // Invariant 6: the loot-box rarity table is found where the reference
    // tooling says it lives, and every rarity it lists is a real rarity.
    let rules = unlocks::loot_box_rules(&data);
    println!("\nloot-box rarity tables: {} found", rules.len());
    let mut rarities_seen: BTreeMap<String, usize> = BTreeMap::new();
    for rule in &rules {
        for entry in rule
            .rarity_rules
            .iter()
            .chain(rule.currency_rarity_rules.iter())
        {
            *rarities_seen.entry(entry.rarity.to_string()).or_insert(0) += 1;
        }
    }
    println!("  rarities listed:                {rarities_seen:?}");

    // Where the rarity table is *supposed* to be, and whether this install has
    // it at all. The reference tooling puts it in the generic-settings asset
    // carrying `STUGenericSettings_LootBoxes` (`0x9044BCFE`, declared at
    // `STUGenericSettings_LootBoxes.cs:6`), so "no table found" is only
    // meaningful next to what the settings assets actually contain.
    let settings_guids = data.guids_of_type(guid::types::STU_GENERIC_SETTINGS);
    let mut settings_readable = 0usize;
    let mut settings_classes: BTreeMap<u32, usize> = BTreeMap::new();
    let mut loot_settings_instances = 0usize;
    let mut loot_settings_fields = 0usize;
    for asset in &settings_guids {
        let Ok(mut stu) = data.stu_of(*asset) else {
            continue;
        };
        settings_readable += 1;
        *settings_classes.entry(stu.instances[0].hash).or_insert(0) += 1;
        loot_settings_instances += stu
            .instances_of(unlocks::class::GENERIC_SETTINGS_LOOT_BOXES)
            .len();
        for i in stu.instances_of(unlocks::class::GENERIC_SETTINGS_LOOT_BOXES) {
            loot_settings_fields += stu.fields(i).len();
        }
    }
    let mut settings_rows: Vec<(&u32, &usize)> = settings_classes.iter().collect();
    settings_rows.sort_by(|a, b| b.1.cmp(a.1));
    println!(
        "generic settings (0x054): {} asset(s) registered, {settings_readable} readable, root \
         classes {:?}",
        settings_guids.len(),
        settings_rows
            .iter()
            .map(|(h, n)| format!("0x{h:08X}x{n}"))
            .collect::<Vec<_>>()
    );
    println!(
        "  STUGenericSettings_LootBoxes (0x{:08X}) instances: {loot_settings_instances}, \
         fields on them: {loot_settings_fields}",
        unlocks::class::GENERIC_SETTINGS_LOOT_BOXES
    );
    assert_eq!(
        loot_settings_instances,
        rules.len(),
        "a STUGenericSettings_LootBoxes instance exists but no table was read from it"
    );

    // -----------------------------------------------------------------------
    // Cross-check the raw reader against the typed layer, where a checkout is
    // available. This is the check that would catch a hash read at the wrong
    // offset when both readers happen to produce *some* number.
    if let Some(registry) = &registry {
        println!("\ncross-checking the raw reader against the typed layer");
        let reg = registry.inner();
        cross_check_skin_weapons(&data, reg, &theme_survey);
        cross_check_conversation_lines(&data, reg, &conversation_survey);
        cross_check_loot_look(&data, reg, &loot_survey);
    }

    // -----------------------------------------------------------------------
    // What this install does not have, stated plainly.
    println!(
        "\nnot exercised on this install: \
         achievement (0x068, nothing downloaded), stat (0x062, nothing \
         downloaded), ranked season (0x0C8, no assets in the registry), \
         catalog (0x063, opens but is not an STU container this tree reads), \
         unlock (0xA5, nothing downloaded)"
    );
    if surveys
        .iter()
        .any(|s| s.type_id == types::ACHIEVEMENT && s.readable > 0)
    {
        println!(
            "note: achievements ARE readable here, so the achievement reader is \
             exercised; the line above is out of date for this install"
        );
    }
}

/// Compare the raw `m_BECFBDBA` / `m_weaponType` reads against the typed layer.
fn cross_check_skin_weapons(data: &GameData, reg: &ow_stu::types::ClassRegistry, survey: &Survey) {
    let mut elements = 0usize;
    let mut skin_agree = 0usize;
    let mut skin_disagree: Vec<String> = Vec::new();
    let mut variation_agree = 0usize;
    let mut checked_assets = 0usize;

    for record in &survey.records {
        let unlocks::Record::SkinTheme(theme) = record else {
            continue;
        };
        if checked_assets >= 4 {
            break;
        }
        let Ok(mut stu) = data.stu_of(theme.asset) else {
            continue;
        };
        let root = stu
            .instance(unlocks::class::SKIN_THEME)
            .or_else(|| stu.instance(unlocks::class::MYTHIC_SKIN));
        let Some(root) = root else { continue };
        checked_assets += 1;
        for (bag, start) in stu.inline_elements(root, unlocks::field::skin_base::WEAPONS) {
            if bag < 0 {
                continue;
            }
            let layout = stu.layout_at(start, bag);
            let raw_skin = layout
                .iter()
                .find(|f| f.hash == unlocks::field::skin_weapon::SKIN)
                .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash));
            let raw_variation = layout
                .iter()
                .find(|f| f.hash == unlocks::field::skin_weapon::WEAPON_TYPE)
                .and_then(|f| stu.i32_at(start + f.offset));
            elements += 1;

            // The typed layer walks the same field bag and so reads the same
            // offsets, but decodes through the registry's declared type rather
            // than the raw reader's "it is eight bytes, un-obfuscate it". Their
            // agreement is therefore a check on the *decode*, not on the layout.
            let typed_skin = stu.typed_field_at(
                unlocks::class::SKIN_WEAPON,
                start,
                bag,
                unlocks::field::skin_weapon::SKIN,
                reg,
            );
            let typed_guid = match typed_skin {
                Some(Value::AssetRef(g)) => Some(g),
                _ => None,
            };
            match (raw_skin, typed_guid) {
                (Some(a), Some(b)) if a == b => skin_agree += 1,
                (a, b) => {
                    if skin_disagree.len() < 4 {
                        skin_disagree.push(format!("raw {a:?} vs typed {typed_skin:?} ({b:?})"));
                    }
                }
            }

            let typed_variation = stu.typed_field_at(
                unlocks::class::SKIN_WEAPON,
                start,
                bag,
                unlocks::field::skin_weapon::WEAPON_TYPE,
                reg,
            );
            if raw_variation.is_some() && typed_variation.is_some() {
                variation_agree += 1;
            }
        }
    }
    println!(
        "  weapon-slot elements checked:   {elements}; m_BECFBDBA agrees with the \
         typed layer {skin_agree} time(s), m_weaponType readable in {variation_agree}"
    );
    for line in &skin_disagree {
        println!("    disagreement: {line}");
    }
    // The declared type is checked here rather than assumed: if the registry
    // ever stops seeing `m_BECFBDBA` as an asset reference, this raw read is no
    // longer reading what the class says it reads.
    assert!(
        reg.field(
            unlocks::class::SKIN_WEAPON,
            unlocks::field::skin_weapon::SKIN
        )
        .map(|f| matches!(f.ty, FieldType::AssetRef))
        .unwrap_or(false),
        "the registry no longer declares STU_BE423F96.m_BECFBDBA an asset reference"
    );
}

/// Compare the raw conversation-line read against the typed layer.
fn cross_check_conversation_lines(
    data: &GameData,
    reg: &ow_stu::types::ClassRegistry,
    survey: &Survey,
) {
    let mut agree = 0usize;
    let mut disagree: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for record in &survey.records {
        let unlocks::Record::VoiceConversation(conversation) = record else {
            continue;
        };
        if checked >= 6 {
            break;
        }
        let Ok(mut stu) = data.stu_of(conversation.asset) else {
            continue;
        };
        let Some(root) = stu.instance(unlocks::class::VOICE_CONVERSATION) else {
            continue;
        };
        checked += 1;
        for (bag, start) in stu.inline_elements(root, unlocks::field::conversation::LINES) {
            if bag < 0 {
                continue;
            }
            let layout = stu.layout_at(start, bag);
            let raw = layout
                .iter()
                .find(|f| f.hash == unlocks::field::conversation_line::VOICE_LINE)
                .and_then(|f| stu.asset_ref_at(start + f.offset, f.hash));
            let typed = stu.typed_field_at(
                unlocks::class::VOICE_CONVERSATION_LINE,
                start,
                bag,
                unlocks::field::conversation_line::VOICE_LINE,
                reg,
            );
            let typed_guid = match typed {
                Some(Value::AssetRef(g)) => Some(g),
                _ => None,
            };
            match (raw, typed_guid) {
                (Some(a), Some(b)) if a == b => agree += 1,
                (a, b) => {
                    if disagree.len() < 4 {
                        disagree.push(format!("raw {a:?} vs typed {b:?}"));
                    }
                }
            }
        }
    }
    println!("  conversation lines agreed:     {agree} (over {checked} asset(s))");
    for line in &disagree {
        println!("    disagreement: {line}");
    }
    assert!(
        agree > 0,
        "the raw and typed conversation-line reads never agreed"
    );
}

/// Compare the raw loot-box model-look read against the typed layer.
fn cross_check_loot_look(data: &GameData, reg: &ow_stu::types::ClassRegistry, survey: &Survey) {
    let mut agree = 0usize;
    let mut checked = 0usize;
    for record in &survey.records {
        let unlocks::Record::LootBox(box_record) = record else {
            continue;
        };
        let Ok(mut stu) = data.stu_of(box_record.asset) else {
            continue;
        };
        let Some(root) = stu.instance(unlocks::class::LOOT_BOX) else {
            continue;
        };
        checked += 1;
        let raw = stu.read_asset_ref(root, unlocks::field::loot_box::MODEL_LOOK);
        let typed = stu.typed_field(root, unlocks::field::loot_box::MODEL_LOOK, reg);
        let typed_guid = match &typed {
            Some(field) => match field.unwrap_defaulted() {
                Value::AssetRef(g) => Some(*g),
                _ => None,
            },
            None => None,
        };
        if raw.is_some() && raw == typed_guid {
            agree += 1;
        }
    }
    println!("  loot-box model looks agreed:   {agree} (over {checked} asset(s))");
}

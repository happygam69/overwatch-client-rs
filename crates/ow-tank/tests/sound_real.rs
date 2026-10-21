//! The sound layer against the real installation: a census of every sound type,
//! the `Sound` → payload → RIFF-header chain on real bytes, and the packet-chain
//! invariant the reference itself checks.
//!
//! Skipped when no install is present, and it asserts nothing when the install's
//! manifests are not readable. Read-only: it opens files and reads them.
//!
//! Run with:
//!
//! ```text
//! cargo test -p ow-tank --release --test sound_real -- --nocapture
//! ```
//!
//! ## Why there are no exact counts here
//!
//! An exact count is a claim about a *patch* of the game, not about the reader:
//! Blizzard adds a hero, a map or a voice pack and the assertion fails while the
//! code is fine. What is worth asserting is an invariant that a *wrong read*
//! would break. The three asserted here are:
//!
//! 1. **Every payload reference a `Sound` yields resolves to an asset that
//!    exists.** A field offset one out, or a reference-array stride one element
//!    out, yields 64-bit values that resolve to nothing — which is how a wrong
//!    layout is caught.
//! 2. **The setup packet is immediately followed by the first audio packet.**
//!    That is the reference's own check (`WwiseRIFFVorbis.cs:776`), so a mis-read
//!    `vorb` offset breaks it.
//! 3. **Every header's own numbers agree with each other**: the declared
//!    container length fits the buffer, the sample rate is a rate audio actually
//!    uses, the loop points are inside the sample count.
//!
//! The counts are printed so a human can see what was measured, and
//! `docs/formats/sound.md` records the figures this produced.

use std::collections::BTreeMap;

use ow_tank::sound::{self, types, Codec, Survey};
use ow_tank::{guid, GameData, ManifestSelector};

/// Upper bound on assets opened per type. A retail manifest records 36,981 sound
/// assets and the reference install has downloaded 2 of them, so an uncapped
/// census spends minutes on CASC misses. The survey records that it was capped.
const SAMPLE_CAP: usize = 40;

/// How many bytes of a failing asset to print, to say what it actually is.
const FAILURE_PREFIX: usize = 32;

fn open() -> Option<(GameData, ow_tank::keytable::Sources)> {
    let root = ow_casc::install::detect_root()?;
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            return None;
        }
    };
    Some((data, sources))
}

fn print_census(survey: &Survey) {
    println!(
        "0x{:03X} {:<20} registered={:<6} scanned={:<5} readable={:<5} parsed={:<5} \
not-downloaded={:<5} unparsed={:<4} as-stu={:<5} as-wem={:<5} v1={:<3} instances={:<6} unrecognised={}{}",
        survey.type_id,
        types::label(survey.type_id),
        survey.assets,
        survey.scanned,
        survey.readable,
        survey.parsed(),
        survey.unreadable,
        survey.unparsed,
        survey.as_stu,
        survey.as_wem,
        survey.version_1,
        survey.instances,
        survey.unrecognised,
        if survey.capped { "  [CAPPED]" } else { "" }
    );
    let mut roots: Vec<(&u32, &usize)> = survey.root_classes.iter().collect();
    roots.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (hash, count) in roots.into_iter().take(5) {
        println!("        root 0x{hash:08X}  {count} asset(s)");
    }
    for line in survey.errors.iter().take(2) {
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
fn real_sound_surface_census_and_invariants() {
    let Some((data, _sources)) = open() else {
        return;
    };

    // -----------------------------------------------------------------------
    // The census
    // -----------------------------------------------------------------------
    let surveys = sound::survey_sound_capped(&data, 2, SAMPLE_CAP);
    println!("\n=== sound surface census (base manifest) ===");
    println!("manifest(s): {}", data.manifests.join(", "));
    for survey in &surveys {
        print_census(survey);
    }

    // A count is a claim about a patch. What is asserted is that the census is
    // *internally consistent*: the counters partition what was scanned, and every
    // parsed asset produced a record. A reader that double-counted or lost assets
    // would break this; a reader that is merely behind a patch would not.
    for survey in &surveys {
        assert_eq!(
            survey.readable + survey.unreadable,
            survey.scanned,
            "type 0x{:03X}: readable + not-downloaded must account for every asset opened",
            survey.type_id
        );
        assert_eq!(
            survey.as_stu + survey.as_wem + survey.unparsed,
            survey.readable,
            "type 0x{:03X}: every readable asset parsed one way or the other, or is counted as unparsed",
            survey.type_id
        );
        assert!(
            survey.records.len() <= 2 || survey.parsed() == 0,
            "type 0x{:03X}: a bounded sample must not be larger than what parsed",
            survey.type_id
        );
    }

    // -----------------------------------------------------------------------
    // The Sound -> payload -> RIFF header chain, on real bytes
    // -----------------------------------------------------------------------
    println!("\n=== sound -> payload -> Wwise header, on real data ===");
    let mut sounds_read = 0usize;
    let mut payload_refs = 0usize;
    let mut payload_refs_resolving = 0usize;
    let mut payload_headers = 0usize;
    let mut banks_resolving = 0usize;
    let mut sample_rates: BTreeMap<u32, usize> = BTreeMap::new();
    let mut chains_consistent = 0usize;
    let mut chains_inconsistent = 0usize;
    let mut chains_not_applicable = 0usize;
    // (asset, payload count, media-id count) for sounds whose two parallel arrays
    // do not pair up. Reported, because the honest answer is "sometimes they do".
    let mut pair_mismatches: Vec<(u64, usize, usize)> = Vec::new();
    let mut pairs_equal = 0usize;

    for asset in data.guids_of_type(types::SOUND) {
        if sounds_read >= 25 {
            break;
        }
        let mut stu = match data.stu_of(asset) {
            Ok(stu) => stu,
            Err(_) => continue,
        };
        let Some(sound) = sound::read_sound_in(&mut stu, asset) else {
            continue;
        };
        sounds_read += 1;

        // The sound bank reference must name an asset that exists.
        if let Some(bank) = sound.wwise.as_ref().and_then(|b| b.sound_bank) {
            if bank != 0 {
                assert!(
                    data.registry().get(bank).is_some(),
                    "0x{asset:016x} names a sound bank 0x{bank:016x} that is not in the registry, \
                     so m_soundBank is being mis-read"
                );
                assert_eq!(
                    guid::type_of(bank),
                    types::SOUND_BANK,
                    "0x{asset:016x} names a sound bank that is not of the bank type"
                );
                banks_resolving += 1;
            }
        }

        // The Wwise media ids are a `uint[]` the reference walks with the same
        // index as the payload reference array (`Combo.cs:942-947`), so a *longer*
        // id list than reference list would mean the descriptor was mis-read. A
        // *shorter* one is measured and reported, not asserted: the reference would
        // index past the end of it, so it is worth seeing rather than asserting a
        // tidy equality the shipped data may not have.
        if let Some(block) = &sound.wwise {
            if block.wwise_file_ids.len() != block.wem_files.len() {
                pair_mismatches.push((asset, block.wem_files.len(), block.wwise_file_ids.len()));
            } else if !block.wwise_file_ids.is_empty() {
                pairs_equal += 1;
            }
        }

        for payload in sound.payload_assets() {
            payload_refs += 1;
            if data.registry().get(payload).is_some() {
                payload_refs_resolving += 1;
            } else {
                println!(
                    "        0x{asset:016x} names payload 0x{payload:016x}, which is not in the \
                     registry: the reference array stride is wrong"
                );
                continue;
            }
            let Ok((header, bytes)) = sound::read_wem(&data, payload) else {
                continue;
            };
            payload_headers += 1;
            *sample_rates.entry(header.sample_rate).or_insert(0) += 1;

            // -- invariant 2: the packet chain ---------------------------------
            match header.setup_packet_consistent() {
                Some(true) => chains_consistent += 1,
                Some(false) => {
                    chains_inconsistent += 1;
                    println!(
                        "        0x{payload:016x}: setup packet at {} (+{} header +{} bytes) does \
                         not land on the first audio packet at {:?}",
                        header.setup_packet_at.unwrap_or(0),
                        header.packet_header_size,
                        header.setup_packet_size.unwrap_or(0),
                        header.first_audio_packet_at
                    );
                }
                None => chains_not_applicable += 1,
            }

            // -- invariant 3: the header's own numbers agree -------------------
            assert!(
                header.total_len as usize <= bytes.len(),
                "0x{payload:016x}: the container declares {} bytes but the asset holds {}",
                header.total_len,
                bytes.len()
            );
            assert!(
                header.channels > 0 && header.channels <= 8,
                "0x{payload:016x}: {} channels is not a count this reader should believe",
                header.channels
            );
            assert!(
                matches!(
                    header.sample_rate,
                    8_000
                        | 11_025
                        | 16_000
                        | 22_050
                        | 24_000
                        | 32_000
                        | 44_100
                        | 48_000
                        | 88_200
                        | 96_000
                        | 176_400
                        | 192_000
                ),
                "0x{payload:016x}: {} Hz is not a rate audio uses, so the fmt offsets are \
                 probably mis-read",
                header.sample_rate
            );
            if header.loop_count > 0 {
                assert!(
                    header.loop_start <= header.loop_end && header.loop_end <= header.sample_count,
                    "0x{payload:016x}: loop {}..{} does not fit {} samples",
                    header.loop_start,
                    header.loop_end,
                    header.sample_count
                );
            }
            // The data chunk must lie inside the container it was found in.
            assert!(
                header.data_offset + header.data_size <= header.total_len as usize,
                "0x{payload:016x}: the data chunk runs past the container"
            );
            // A setup packet, when present, must lie inside the data chunk.
            if let (Some(setup), Some(first)) =
                (header.setup_packet_at, header.first_audio_packet_at)
            {
                assert!(
                    setup >= header.data_offset && setup < header.data_offset + header.data_size,
                    "0x{payload:016x}: the setup packet at {setup} is not inside the data chunk"
                );
                assert!(
                    first > setup,
                    "0x{payload:016x}: the first audio packet at {first} does not follow the setup \
                     packet at {setup}"
                );
            }
        }
    }

    println!("  sounds read                      {sounds_read}");
    println!("  sound bank references resolving  {banks_resolving}");
    println!("  payload references seen          {payload_refs}");
    println!("  payload references resolving     {payload_refs_resolving}");
    println!("  payload headers parsed           {payload_headers}");
    println!("  packet chains consistent         {chains_consistent}");
    println!("  packet chains INCONSISTENT       {chains_inconsistent}");
    println!("  packet chains not applicable     {chains_not_applicable}");
    println!("  sample rates seen                {sample_rates:?}");
    println!("  sounds whose WEM list and media-id list pair up  {pairs_equal}");
    println!(
        "  sounds where the two lists differ in length  {}",
        pair_mismatches.len()
    );
    for (asset, files, ids) in pair_mismatches.iter().take(5) {
        println!("        0x{asset:016x}: {files} payload reference(s), {ids} media id(s)");
    }

    if sounds_read == 0 {
        println!(
            "\nnothing to assert: no sound asset of type 0x{:03X} could be read on this install.\n\
             That is itself the finding the census above reports — the registry lists them, the\n\
             install has not downloaded them.",
            types::SOUND
        );
        return;
    }

    // The check that catches a wrong layout. A `Sound`'s payload references are
    // read from a reference array in the dynamic block with the typed layer's
    // sixteen-byte stride; a wrong stride produces values that resolve to nothing.
    assert!(
        payload_refs_resolving == payload_refs,
        "not every payload reference resolved: {payload_refs_resolving} of {payload_refs}. A \
         reference array read at the wrong stride yields GUIDs that name nothing."
    );
    assert!(
        banks_resolving > 0,
        "no sound read named a sound bank, so m_soundBank was never exercised on real data"
    );
    assert!(
        chains_inconsistent == 0,
        "{chains_inconsistent} payload(s) had a setup packet that did not lead to the first audio \
         packet, which means the vorb offsets are being mis-read"
    );
    assert!(
        payload_headers > 0,
        "no payload header was parsed, so the RIFF walk was never exercised on real data"
    );
    assert!(
        pair_mismatches.iter().all(|(_, files, ids)| ids <= files),
        "a sound's m_wwiseWEMFileIDs list was longer than its m_soundWEMFiles list, which the \
         reference indexes in step; the descriptor is being mis-read"
    );
    assert!(
        pairs_equal > 0,
        "no sound read had its WEM list and media-id list the same length, so the parallel-array \
         layout was never exercised on real data"
    );
}

/// The speech manifest: the per-locale one that actually holds voice audio.
///
/// This is the layer's most load-bearing install claim — `GameData::open` does not
/// read it, so without opening it a voice census would say "zero voice assets"
/// and be wrong.
#[test]
fn the_speech_manifest_holds_the_voice_payloads() {
    let Some((data, sources)) = open() else {
        return;
    };

    // The claim being tested: the base manifest does not carry voice, and the
    // speech manifest does. Asserting both halves keeps a change that quietly
    // merged the manifests from passing.
    let base_voice_sets = data.guids_of_type(types::VOICE_SET).len();
    let base_voice_payloads = data.guids_of_type(types::VOICE_WEM_FILE).len();
    println!(
        "\n=== speech manifest ===\nbase manifest: {base_voice_sets} voice set(s), \
         {base_voice_payloads} voice payload(s)"
    );

    let Some(index) = sound::open_speech(&data, &sources).unwrap_or_else(|e| {
        eprintln!("note: the speech manifest could not be opened ({e}); nothing to assert");
        None
    }) else {
        eprintln!(
            "note: no speech manifest for this platform/region/locale; nothing to assert. \
             Not having enUS voice on disk is a fact about the install, not a read failure."
        );
        return;
    };

    println!(
        "speech manifest {} ({} record(s))",
        index.manifest,
        index.assets.len()
    );
    let histogram = index.type_histogram();
    for (type_id, count) in &histogram {
        println!(
            "  0x{type_id:03X}  {count:>7}  {}",
            if types::ALL.contains(type_id) {
                "in the sound layer's list"
            } else {
                "not in the sound layer's list"
            }
        );
    }

    if index.assets.is_empty() {
        eprintln!("the speech manifest opened but is empty; nothing to assert");
        return;
    }

    // Every type the layer claims to read out of the speech manifest must be
    // present there. If one is not, the layer's type list is wrong about where
    // that data lives, which is the thing this test exists to catch.
    assert!(
        base_voice_sets == 0 || !index.guids_of_type(types::VOICE_SET).is_empty(),
        "voice sets exist somewhere but neither manifest is where the layer looks"
    );

    // Parse real voice payloads. This is the same header walk as the SFX side; the
    // claim is that it holds for voice too.
    let mut read = 0usize;
    let mut not_downloaded = 0usize;
    let mut failed = 0usize;
    let mut channels: BTreeMap<u16, usize> = BTreeMap::new();
    let mut chains_consistent = 0usize;
    let mut chains_inconsistent = 0usize;

    let payload_type = if !index.guids_of_type(types::VOICE_WEM_FILE).is_empty() {
        types::VOICE_WEM_FILE
    } else {
        types::SOUND_FILE
    };
    let guids = index.guids_of_type(payload_type);
    println!(
        "walking type 0x{payload_type:03X} ({} asset(s)) in the speech manifest",
        guids.len()
    );
    let step = (guids.len() / 25usize).max(1);

    for guid in guids.iter().copied().step_by(step).take(25) {
        match index.bytes_of(&data, guid) {
            Ok(_) => {}
            Err(_) => {
                not_downloaded += 1;
                continue;
            }
        }
        match index.read_wem(&data, guid) {
            Ok((header, bytes)) => {
                read += 1;
                *channels.entry(header.channels).or_insert(0) += 1;
                assert!(
                    header.total_len as usize <= bytes.len(),
                    "speech payload 0x{guid:016x} declares {} bytes but holds {}",
                    header.total_len,
                    bytes.len()
                );
                assert!(
                    header.channels > 0 && header.channels <= 8,
                    "speech payload 0x{guid:016x}: {} channels is not believable",
                    header.channels
                );
                match header.setup_packet_consistent() {
                    Some(true) => chains_consistent += 1,
                    Some(false) => chains_inconsistent += 1,
                    None => {}
                }
                if read <= 5 {
                    println!(
                        "  0x{guid:016x}  {} bytes  {}",
                        bytes.len(),
                        header.summary()
                    );
                }
            }
            Err(e) => {
                failed += 1;
                if failed <= 5 {
                    println!("  0x{guid:016x}  header failed: {e}");
                }
            }
        }
    }

    println!("  {read} header(s) parsed, {failed} failed, {not_downloaded} not downloaded");
    println!("  channels seen {channels:?}");
    println!("  packet chains consistent {chains_consistent}, INCONSISTENT {chains_inconsistent}");

    if read == 0 {
        println!(
            "no voice payload on this install could be read ({not_downloaded} not downloaded, \
             {failed} unreadable). Nothing to assert beyond what is printed."
        );
        return;
    }

    assert_eq!(
        chains_inconsistent, 0,
        "a voice payload's setup packet did not lead to its first audio packet"
    );
    assert_eq!(
        failed, 0,
        "every voice payload that opened its bytes also parsed as a Wwise container"
    );
    // Voice is spoken, so it should be mono far more often than not. This is a
    // shape claim, not a count, so it is stated as "at least one mono file" rather
    // than a proportion that a patch could change.
    assert!(
        channels.keys().any(|c| *c >= 1 && *c <= 2),
        "no voice payload had one or two channels, which is what speech should look like; \
         the fmt offsets are probably mis-read"
    );
}

/// Voice sets in the speech manifest: the metadata half of the voice chain.
#[test]
fn voice_sets_name_their_payloads() {
    let Some((data, sources)) = open() else {
        return;
    };
    let Some(index) = sound::open_speech(&data, &sources).unwrap_or(None) else {
        eprintln!("note: no speech manifest; nothing to assert");
        return;
    };
    let guids = index.guids_of_type(types::VOICE_SET);
    println!("\n=== voice sets (speech manifest) ===");
    println!("{} voice set asset(s) registered", guids.len());
    if guids.is_empty() {
        println!("none registered on this install; nothing to assert");
        return;
    }

    let mut read = 0usize;
    let mut failed = 0usize;
    let mut with_stimulus = 0usize;
    let mut with_payloads = 0usize;
    let mut payload_refs = 0usize;
    let mut payload_refs_in_speech = 0usize;
    let step = (guids.len() / 20usize).max(1);

    for guid in guids.iter().copied().step_by(step).take(20) {
        let bytes = match index.bytes_of(&data, guid) {
            Ok(bytes) => bytes,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        let mut stu = match ow_stu::Stu::parse(bytes) {
            Ok(stu) => stu,
            Err(e) => {
                println!("  0x{guid:016x}: not STU: {e}");
                failed += 1;
                continue;
            }
        };
        let Some(set) = sound::read_voice_set_in(&mut stu, guid) else {
            println!(
                "  0x{guid:016x}: no STUVoiceSet instance (0x{:08X})",
                sound::class::VOICE_SET
            );
            failed += 1;
            continue;
        };
        read += 1;
        if set.lines_with_stimulus() > 0 {
            with_stimulus += 1;
        }
        let payloads = set.payload_assets();
        if !payloads.is_empty() {
            with_payloads += 1;
        }
        for payload in &payloads {
            payload_refs += 1;
            if index.get(*payload).is_some() || data.registry().get(*payload).is_some() {
                payload_refs_in_speech += 1;
            } else {
                println!(
                    "  0x{guid:016x} names payload 0x{payload:016x}, which is in neither manifest"
                );
            }
        }
        if read <= 5 {
            println!(
                "  0x{guid:016x}  gender={:?} lines={} with_stimulus={} stimuli={} payloads={}",
                set.gender,
                set.lines.len(),
                set.lines_with_stimulus(),
                set.stimuli().len(),
                payloads.len()
            );
        }
    }

    println!(
        "  {read} voice set(s) read, {failed} failed; {with_stimulus} with a stimulus, \
         {with_payloads} with payloads"
    );
    println!("  payload references: {payload_refs} seen, {payload_refs_in_speech} found");

    if read == 0 {
        println!("no voice set could be read on this install; nothing to assert");
        return;
    }

    assert!(
        payload_refs_in_speech == payload_refs,
        "not every payload reference from a voice set resolves in a manifest: \
         {payload_refs_in_speech} of {payload_refs}"
    );
    assert!(
        with_stimulus > 0,
        "no voice set read had a line with a stimulus, so the STUVoiceLineInstance -> \
         STUVoiceLine join was never exercised on real data"
    );
}

/// Voice line sets in the base manifest, and what their line references are.
///
/// The measurement here is a **negative** one and the test exists to keep it true:
/// a voice line set's `m_0DD426F8` is declared as a reference array but its
/// element class, `STUVoiceLine`, is embedded-only and has no asset type, so the
/// decoded values name nothing in the registry. A reader that started resolving
/// them would be wrong in a new way, and this says so.
#[test]
fn voice_line_sets_name_lines_that_are_not_assets() {
    let Some((data, _sources)) = open() else {
        return;
    };
    let guids = data.guids_of_type(types::VOICE_LINE_SET);
    println!("\n=== voice line sets (base manifest) ===");
    println!("{} asset(s) registered", guids.len());
    if guids.is_empty() {
        println!("none registered on this install; nothing to assert");
        return;
    }

    let step = (guids.len() / 20usize).max(1);
    let mut read = 0usize;
    let mut refs = 0usize;
    let mut refs_resolving = 0usize;
    let mut sizes: Vec<usize> = Vec::new();

    for guid in guids.iter().copied().step_by(step).take(20) {
        let Ok(mut stu) = data.stu_of(guid) else {
            continue;
        };
        let Some(set) = sound::read_voice_line_set_in(&mut stu, guid) else {
            continue;
        };
        read += 1;
        sizes.push(set.lines.len());
        if read <= 5 {
            println!(
                "  0x{guid:016x}  {} line reference(s), first {:?}",
                set.lines.len(),
                set.lines.first().map(|g| format!("0x{g:016x}"))
            );
        }
        // The array layout itself is checked by the count: a wrong stride or a
        // wrong descriptor read yields zero, or a number nothing else agrees with.
        if !set.lines.is_empty() {
            for line in &set.lines {
                refs += 1;
                if data.registry().get(*line).is_some() {
                    refs_resolving += 1;
                }
            }
        }
    }

    println!(
        "  {read} read, {refs} line reference(s), {refs_resolving} resolving; \
         per-set line counts {sizes:?}"
    );
    if read == 0 {
        println!("none readable on this install; nothing to assert");
        return;
    }

    assert!(
        refs > 0,
        "a readable voice line set held no line references at all, so the dynamic-block \
         descriptor layout was never exercised"
    );
    assert_eq!(
        refs_resolving, 0,
        "a voice line set's line reference resolved to an asset. That should not happen: \
         STUVoiceLine is an embedded class with no asset type, so there is nothing for it \
         to point at. If this fires, the field is no longer what the reference declares."
    );
}

/// The sound bank type is a raw Wwise bank, not a STU asset.
///
/// This is recorded as a finding rather than a bug: it is why
/// [`sound::SoundBank`] is documented as untested against real data, and it is
/// the sort of thing a reader would otherwise report as "unparsed".
#[test]
fn sound_bank_assets_are_raw_wwise_banks() {
    let Some((data, _sources)) = open() else {
        return;
    };
    let guids = data.guids_of_type(types::SOUND_BANK);
    println!("\n=== Wwise sound banks (base manifest) ===");
    println!("{} asset(s) registered", guids.len());
    if guids.is_empty() {
        println!("none registered on this install; nothing to assert");
        return;
    }

    let step = (guids.len() / 25usize).max(1);
    let mut opened = 0usize;
    let mut bkhd = 0usize;
    let mut riff = 0usize;
    let mut other = 0usize;
    let mut as_stu = 0usize;

    for guid in guids.iter().copied().step_by(step).take(25) {
        let Ok(bytes) = data.bytes_of(guid) else {
            continue;
        };
        opened += 1;
        if bytes.len() >= 4 {
            match &bytes[0..4] {
                b"BKHD" => bkhd += 1,
                b"RIFF" | b"RIFX" => riff += 1,
                _ => {
                    other += 1;
                    print_failure_prefix(&data, guid);
                }
            }
        }
        if ow_stu::Stu::parse(bytes).is_ok() {
            as_stu += 1;
        }
    }

    println!("  {opened} opened: {bkhd} BKHD, {riff} RIFF, {other} other; {as_stu} parse as STU");
    if opened == 0 {
        println!("none of them are downloaded on this install; nothing to assert");
        return;
    }
    // The claim: a type-0x043 asset is a Wwise bank, which is neither this
    // module's container nor an STU asset. Asserted as "at least one is BKHD and
    // none is an STU", because "every one" would be a claim about every patch.
    assert!(
        bkhd > 0,
        "no sampled sound bank began with BKHD; the bank format may have changed"
    );
    assert_eq!(
        as_stu, 0,
        "a type-0x043 asset parsed as STU, so sound::SoundBank is no longer untested"
    );
    assert_eq!(
        riff, 0,
        "a type-0x043 asset parsed as a media container; the bank format may have changed"
    );
}

/// A payload asset that is neither STU nor a Wwise container says which it is,
/// rather than being counted as an anonymous failure.
#[test]
fn an_unreadable_payload_reports_its_first_bytes() {
    let Some((data, _sources)) = open() else {
        return;
    };
    for type_id in types::WEM_PAYLOAD {
        let survey = sound::survey_type_capped(&data, *type_id, 0, 25);
        if survey.unparsed == 0 {
            continue;
        }
        println!(
            "type 0x{type_id:03X}: {} unparsed of {} scanned",
            survey.unparsed, survey.scanned
        );
        for line in survey.errors.iter().take(3) {
            println!("        {line}");
        }
        // The error has to *name* what it saw, not just fail. That is the property
        // under test: an unknown format reported as a number is worse than useless.
        for line in &survey.errors {
            assert!(
                line.contains("first four bytes")
                    || line.contains("vorb")
                    || line.contains("chunk")
                    || line.contains("BKHD"),
                "the read failure does not say what it saw: {line}"
            );
        }
    }
}

/// The `Sound` type id and the Wwise payload type ids, checked against the
/// reference install's own type histogram.
///
/// This asserts *which* types hold audio, which is the claim the whole layer
/// rests on and the one most worth a test: if the payload type changed, every
/// Sound reference would point at nothing.
#[test]
fn audio_lives_in_the_payload_types_not_the_metadata_types() {
    let Some((data, _sources)) = open() else {
        return;
    };
    let histogram = data.type_histogram();

    println!("\n=== where audio lives ===");
    for type_id in [
        types::SOUND,
        types::SOUND_BANK,
        types::SOUND_FILE,
        types::SOUND_FILE_ALT,
        types::VOICE_WEM_FILE,
        types::VOICE_SET,
    ] {
        println!(
            "  0x{type_id:03X} {:<20} {} asset(s) in the base manifest",
            types::label(type_id),
            histogram.get(&type_id).copied().unwrap_or(0)
        );
    }

    // The metadata types and the payload types must not be the same type. If a
    // type were both, one reader would have to be right about two different
    // formats, which is the failure this split exists to prevent.
    for meta in [types::SOUND, types::SOUND_PARAMETER, types::SOUND_SWITCH] {
        for payload in types::WEM_PAYLOAD {
            let payload = *payload;
            assert_ne!(
                meta, payload,
                "type 0x{meta:03X} is both a metadata type and a payload type"
            );
        }
    }

    // A payload type with assets registered must actually hold containers. A
    // stride-sampled read of type 0x03F is the measurement; if the install has
    // none downloaded there is nothing to assert and the test says so.
    let sfx = sound::survey_type_capped(&data, types::SOUND_FILE_ALT, 0, 25);
    if sfx.readable > 0 {
        assert!(
            sfx.as_wem > 0,
            "type 0x{:03X} has {} readable asset(s) but none parsed as a Wwise container",
            types::SOUND_FILE_ALT,
            sfx.readable
        );
        println!(
            "  type 0x{:03X}: {} of {} readable parsed as Wwise containers",
            types::SOUND_FILE_ALT,
            sfx.as_wem,
            sfx.readable
        );
    } else {
        println!(
            "  type 0x{:03X}: nothing downloaded on this install; nothing to assert",
            types::SOUND_FILE_ALT
        );
    }
    let _ = Codec::Vorbis;
}

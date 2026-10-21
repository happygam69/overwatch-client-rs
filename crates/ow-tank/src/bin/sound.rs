//! The sound asset layer against a real installation: a census of every sound
//! type, the Wwise RIFF header walk on real payloads, and the handoff that
//! writes a payload out for an external decoder.
//!
//! Subcommands:
//!
//! ```text
//! ow-tank-sound types                       # census every sound type
//! ow-tank-sound types --cap 40 --limit 4    # cap how many assets are opened
//! ow-tank-sound dump sound --limit 5        # print records of one type
//! ow-tank-sound wem 0x000000B2...            # one payload's RIFF header
//! ow-tank-sound wem 0x... --out out\        # ...and write payload + sidecar
//! ow-tank-sound payloads sound --limit 3     # follow Sound -> WEM -> header
//! ow-tank-sound speech --cap 5              # the per-locale speech manifest
//! ```
//!
//! Read-only with respect to the install: it opens files and reads them. The
//! `--out` directory is the only thing it writes, and only when asked.
//!
//! **It does not decode audio.** The `wem` subcommand writes the raw payload and
//! a sidecar describing the header; turning that into sound is a separate,
//! external step. See `docs/formats/sound.md`.

use std::path::PathBuf;

use ow_tank::sound::{self, types, Codec, Record, Sound, Survey, WemHeader};
use ow_tank::{guid, GameData, ManifestSelector};

const USAGE: &str = "\
usage: ow-tank-sound <subcommand>

  types                       census every sound type against the install
  dump <type>                 print sample records of one type
  wem <0xGUID>                one payload's Wwise RIFF header
  payloads <type>             follow Sound -> WEM payload -> header, on real data
  speech                      the per-locale speech manifest (voice lives there)

common flags:
  --cap N        open at most N assets per type, stride-sampled; 0 means no cap
                 (default 40, which is a sample and not a census — the output says
                 [CAPPED] when it applies)
  --limit N      print at most N records per type (default 3)
  --out DIR      with `wem`: write <GUID>.wem and <GUID>.wem.txt into DIR
  --manifest S   force a manifest by substring (OW_MANIFEST does the same)

type names: sound, sound-switch, sound-switch-group, sound-parameter,
sound-state-group, sound-state, sound-space, voice-set, voice-line-set,
sound-integrity, voice-wem-file, sound-bank, wem-file, wem-file-alt
(literal 0x0B2-style ids work too)";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(|s| s.as_str()) else {
        eprintln!("{USAGE}");
        return std::process::ExitCode::from(2);
    };
    match command {
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            std::process::ExitCode::SUCCESS
        }
        "types" => cmd_types(&args[1..]),
        "dump" => cmd_dump(&args[1..]),
        "wem" => cmd_wem(&args[1..]),
        "payloads" => cmd_payloads(&args[1..]),
        "speech" => cmd_speech(&args[1..]),
        other => {
            eprintln!("unknown subcommand {other:?}\n");
            eprintln!("{USAGE}");
            std::process::ExitCode::from(2)
        }
    }
}

/// Read the flag `--name`, and the value after it, as a `usize`.
fn flag_usize(args: &[String], name: &str, default: usize) -> usize {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read the flag `--name`, and the value after it, as a `PathBuf`.
fn flag_path(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

/// Open the install, or say why not and return `None`.
fn open() -> Option<GameData> {
    open_with_sources().map(|(data, _)| data)
}

/// Open the install and hand back the key sources too, for the subcommands that
/// need to open a second manifest (the speech one).
fn open_with_sources() -> Option<(GameData, ow_tank::keytable::Sources)> {
    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL to point at one");
            return None;
        }
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    match GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(data) => Some((data, sources)),
        Err(e) => {
            eprintln!("open failed: {e}");
            None
        }
    }
}

/// One census line per type, plus the shapes behind it.
fn print_survey(survey: &Survey, records: bool) {
    println!(
        "type 0x{:03X} {:<26} registered={:<6} scanned={:<5} readable={:<5} parsed={:<5} \
not-downloaded={:<5} unparsed={:<4} as-stu={:<5} as-wem={:<5} v1={:<3} instances={}{}",
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
        if survey.capped {
            "  [CAPPED - not a complete census]"
        } else {
            ""
        }
    );
    if survey.unrecognised > 0 {
        println!(
            "        {} asset(s) parsed with a root class this module has no reader for",
            survey.unrecognised
        );
    }
    if !survey.missing_sample.is_empty() {
        let list: Vec<String> = survey
            .missing_sample
            .iter()
            .map(|g| format!("0x{g:016x}"))
            .collect();
        println!(
            "        {} asset(s) are in the registry but not on disk, e.g. {}",
            survey.unreadable,
            list.join(", ")
        );
    }
    let mut roots: Vec<(&u32, &usize)> = survey.root_classes.iter().collect();
    roots.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (hash, count) in roots.into_iter().take(6) {
        println!("        root 0x{hash:08X}  {count} asset(s)");
    }
    if !records {
        return;
    }
    for record in &survey.records {
        println!("        0x{:016x}  {}", record.asset(), record.summary());
    }
    for line in survey.errors.iter().take(4) {
        println!("        error: {line}");
    }
}

fn cmd_types(args: &[String]) -> std::process::ExitCode {
    let Some(data) = open() else {
        return std::process::ExitCode::from(2);
    };
    let cap_arg = flag_usize(args, "--cap", 40);
    let cap = if cap_arg == 0 { usize::MAX } else { cap_arg };
    let limit = flag_usize(args, "--limit", 3);
    println!(
        "\nsound surface census — up to {cap} asset(s) per type, up to {limit} record(s) printed\n"
    );
    println!(
        "manifest(s): {}\n",
        data.manifests
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let surveys = sound::survey_sound_capped(&data, limit, cap);
    for survey in &surveys {
        print_survey(survey, true);
    }

    // A row for the payload types that turned out to hold something else, because
    // that is the finding a reader most wants stated plainly. The codec and chunk
    // tallies come from walking the same sample a second time, which is cheap
    // because the assets are already in the cache.
    println!("\ndeclared payload types and what they actually are:");
    for t in types::WEM_PAYLOAD {
        let survey = sound::survey_type_capped(&data, *t, 0, cap);
        println!(
            "  0x{:03X} {:<18} declared a WEM container; measured: {} STU, {} RIFF, {} neither, {} not downloaded",
            t,
            types::label(*t),
            survey.as_stu,
            survey.as_wem,
            survey.unparsed,
            survey.unreadable
        );
        if survey.as_wem > 0 {
            for (shape, count) in payload_shapes(&data, *t, cap) {
                println!("        {count:>5}  {shape}");
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

/// What the payload assets of one type actually look like inside, tallied.
///
/// Sorted by name so two runs of the same command produce the same output. The
/// labels are deliberately plain — this is a shape census, not a decoder.
fn payload_shapes(data: &GameData, type_id: u16, cap: usize) -> Vec<(String, usize)> {
    let guids = data.guids_of_type(type_id);
    let stride = (guids.len() / cap.max(1)).max(1);
    let mut tally: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for guid in guids.into_iter().step_by(stride).take(cap) {
        let Ok(bytes) = data.bytes_of(guid) else {
            continue;
        };
        let Ok(header) = sound::parse_wem(&bytes) else {
            continue;
        };
        let mut label = format!(
            "{} {} Hz {} ch",
            header.codec, header.sample_rate, header.channels
        );
        if header.codec == Codec::Vorbis {
            label.push_str(&format!(" fmt {:#x}", header.fmt_size));
            label.push_str(if header.vorb_synthesised {
                " no-vorb"
            } else {
                " vorb"
            });
            label.push_str(&format!(" {}B packet header", header.packet_header_size));
            if let Some(ok) = header.ext_signature_ok {
                label.push_str(if ok {
                    " fmt signature ok"
                } else {
                    " FMT SIGNATURE MISMATCH"
                });
            }
        }
        if header.mapping_family.is_some() {
            label.push_str(&format!(
                " mapping family {:?}",
                header.mapping_family.unwrap_or(0)
            ));
        }
        if header.loop_count > 0 {
            label.push_str(" loops");
        }
        let unknown = header.unknown_chunks();
        if !unknown.is_empty() {
            let tags: Vec<String> = unknown.iter().map(|c| c.tag()).collect();
            label.push_str(&format!(" extra chunk(s) {}", tags.join(",")));
        }
        *tally.entry(label).or_insert(0) += 1;
    }
    tally.into_iter().collect()
}

fn cmd_dump(args: &[String]) -> std::process::ExitCode {
    let Some(data) = open() else {
        return std::process::ExitCode::from(2);
    };
    let Some(name) = args.first() else {
        eprintln!("dump wants a type name\n\n{USAGE}");
        return std::process::ExitCode::from(2);
    };
    let Some(type_id) = types::by_name(name) else {
        eprintln!("{name:?} is not a sound type this tool knows; see the list in --help");
        return std::process::ExitCode::from(2);
    };
    let limit = flag_usize(args, "--limit", 10);
    // Same default as `types`: a retail manifest records tens of thousands of
    // sound assets and downloads almost none of them, so walking all of them is
    // minutes of cache misses. `--cap 0` means no cap.
    let cap_arg = flag_usize(args, "--cap", 40);
    let cap = if cap_arg == 0 { usize::MAX } else { cap_arg };

    let survey = sound::survey_type_capped(&data, type_id, limit, cap);
    println!(
        "\ntype 0x{type_id:03X} {} — {} registered, {} scanned, {} readable, {} parsed \
(as-stu {}, as-wem {})\n",
        types::label(type_id),
        survey.assets,
        survey.scanned,
        survey.readable,
        survey.parsed(),
        survey.as_stu,
        survey.as_wem
    );
    if survey.capped {
        println!("NOTE: capped at {cap} asset(s); the counters describe the sample, not the type.");
    }
    for record in &survey.records {
        println!("0x{:016x}", record.asset());
        println!("    {}", record.summary());
        if let Record::Sound(sound) = record {
            print_sound_detail(sound);
        }
        if let Record::VoiceSet(set) = record {
            print_voice_set_detail(set);
        }
    }
    for line in &survey.errors {
        println!("error: {line}");
    }
    if survey.records.is_empty() && survey.assets > 0 {
        println!(
            "\nnothing read: {} asset(s) registered, {} not downloaded. An asset in the \
             registry but not on disk is normal — Blizzard ships the manifest for \
             everything and downloads only what you have played.",
            survey.assets, survey.unreadable
        );
        if !survey.missing_sample.is_empty() {
            let list: Vec<String> = survey
                .missing_sample
                .iter()
                .map(|g| format!("0x{g:016x}"))
                .collect();
            println!(
                "these are real GUIDs this install simply does not have: {}",
                list.join(", ")
            );
            println!(
                "try one with `ow-tank-sound wem {}` to see that reported.",
                list[0]
            );
        }
    }
    std::process::ExitCode::SUCCESS
}

fn print_sound_detail(sound: &Sound) {
    if let Some(block) = &sound.wwise {
        println!(
            "      wwise block: name={:?} play_event={:?} stop_event={:?} bank=0x{:016x}",
            block.name,
            block.event_play,
            block.event_stop,
            block.sound_bank.unwrap_or(0)
        );
        println!(
            "        pre-loaded payloads: {} (with {} Wwise media id(s))",
            block.wem_files.len(),
            block.wwise_file_ids.len()
        );
        for (id, guid) in block.files_with_ids().iter().take(6) {
            println!("          wwise media id {id} -> 0x{guid:016x}");
        }
        if block.wem_streams.is_empty() {
            println!("        streamed payloads: none");
        } else {
            println!(
                "        streamed payloads: {} (with {} Wwise media id(s))",
                block.wem_streams.len(),
                block.wwise_stream_ids.len()
            );
        }
    } else {
        println!("      wwise block: none — this sound names no media of its own");
    }
    // Every `teString` the asset's field bag declares. On the reference install the
    // sound surface declares almost none, so an empty line here is the measurement,
    // not a failure to look.
    if sound.numbers.strings.is_empty() {
        println!("      strings on this sound: none declared");
    } else {
        for (hash, text) in &sound.numbers.strings {
            println!("      string 0x{hash:08X} = {text:?}");
        }
    }
    // A few of the fields the reference declares, so the dump is a check on the
    // hashes rather than only a count.
    println!(
        "      unnamed: m_D138B819={:?} m_0BF329D0={:?} m_6657412F={:?} m_EF90634C={:?} m_B62657B8={:?}",
        sound.numbers.f32(sound::field::sound::UNNAMED_D138B819),
        sound.numbers.f32(sound::field::sound::UNNAMED_0BF329D0),
        sound.numbers.u32(sound::field::sound::UNNAMED_6657412F),
        sound
            .numbers
            .u32(sound::field::sound::UNNAMED_EF90634C)
            .map(|v| format!("{v} (0x{v:08X})")),
        sound.numbers.u8(sound::field::sound::UNNAMED_B62657B8),
    );
    println!(
        "      other numeric fields declared: {} word(s), {} byte(s)",
        sound.numbers.words.len(),
        sound.numbers.bytes.len()
    );
}

fn print_voice_set_detail(set: &sound::VoiceSet) {
    println!(
        "      {} line(s), {} with a stimulus, {} distinct stimulus/stimuli, {} distinct payload(s)",
        set.lines.len(),
        set.lines_with_stimulus(),
        set.stimuli().len(),
        set.payload_assets().len()
    );
    for (i, line) in set.lines.iter().take(4).enumerate() {
        let stimulus = line.line.as_ref().and_then(|l| l.stimulus).unwrap_or(0);
        println!(
            "        line {i}: stimulus=0x{stimulus:016x} weight={:?} payloads={} external_sound=0x{:016x}",
            line.line.as_ref().and_then(|l| l.weight),
            line.payloads.len(),
            line.line.as_ref().and_then(|l| l.external_sound).unwrap_or(0)
        );
    }
    if set.lines.len() > 4 {
        println!("        ... and {} more line(s)", set.lines.len() - 4);
    }
}

/// Print one WEM header, field by field, the way a decoder needs to see it.
fn print_header(header: &WemHeader) {
    println!(
        "  codec            {} (fmt word 0x{:04X})",
        header.codec,
        header.codec.word()
    );
    println!("  channels         {}", header.channels);
    println!("  sample rate      {} Hz", header.sample_rate);
    println!(
        "  avg bytes/sec    {} (the reference turns this into a nominal bitrate)",
        header.avg_bytes_per_second
    );
    println!(
        "  samples          {}",
        match header.codec {
            // Opus keeps its sample count in the `fmt ` chunk; `sample_count` is
            // the Vorbis `vorb` field and is zero for Opus, so printing it would be
            // printing a field that does not apply.
            ow_tank::sound::Codec::Opus =>
                "not applicable to Opus (see 'opus samples')".to_string(),
            _ => format!(
                "{} per channel ({})",
                header.sample_count,
                match header.duration_seconds() {
                    Some(s) => format!("~{s:.3} s, derived from the sample count and rate"),
                    None => "length unknown".to_string(),
                }
            ),
        }
    );
    if let Some(n) = header.num_samples {
        println!(
            "  opus samples     {n} ({})",
            match header.duration_seconds() {
                Some(s) => format!("~{s:.3} s"),
                None => "length unknown".into(),
            }
        );
    }
    println!(
        "  fmt chunk        {:#x} bytes, subtype {:#x} (the reference comments this is the channel layout), ext word {:#x}",
        header.fmt_size, header.subtype, header.ext_unk
    );
    if let Some(ok) = header.ext_signature_ok {
        println!(
            "  fmt signature    {}",
            if ok {
                "matches the reference's expected 16 bytes"
            } else {
                "DOES NOT match — this is not a layout the reference has seen"
            }
        );
    }
    println!(
        "  endianness       {}",
        if header.big_endian {
            "big (RIFX)"
        } else {
            "little (RIFF)"
        }
    );
    println!(
        "  container        {} bytes declared by the RIFF size field",
        header.total_len
    );
    println!("  packet header    {} bytes", header.packet_header_size);
    println!(
        "  packet quirks    no_granule={} mod_packets={} header_triad={} old_headers={}",
        header.no_granule,
        header.mod_packets,
        header.header_triad_present,
        header.old_packet_headers
    );
    if header.vorb_synthesised {
        println!("  vorb chunk       NONE in the file; the reference assumes one at fmt + 0x18");
    }
    if let Some(uid) = header.uid {
        println!("  stream uid       0x{uid:08x}");
    }
    if let (Some(a), Some(b)) = (header.blocksize0_pow, header.blocksize1_pow) {
        println!("  blocksizes       pow {a} and {b}");
    }
    println!(
        "  loop             {}",
        if header.loop_count == 0 {
            "none".to_string()
        } else {
            format!(
                "{} loop, samples {}..{} (already ordered and clamped)",
                header.loop_count, header.loop_start, header.loop_end
            )
        }
    );
    if let Some(c) = header.cue_count {
        println!("  cues             {c}");
    }
    println!(
        "  data chunk       offset {}, {} bytes",
        header.data_offset, header.data_size
    );
    match header.codec {
        Codec::Vorbis => {
            println!(
                "  setup packet     offset {} (relative to data), {} bytes at absolute {}",
                header
                    .setup_packet_offset
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                header
                    .setup_packet_len()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                header
                    .setup_packet_at
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".into()),
            );
            println!(
                "  first audio pkt  absolute {}",
                header
                    .first_audio_packet_at
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".into())
            );
        }
        Codec::Opus => {
            println!(
                "  seek table       {} frame size(s), codec version {}",
                header
                    .seek_table_count
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".into()),
                match header.codec_version {
                    Some(v) => v.to_string(),
                    None => "unknown".into(),
                }
            );
        }
        Codec::Other(word) => {
            println!("  codec word       {word:#06x}, which this reader does not decode")
        }
    }
    println!("  chunks           {}", header.chunks.len());
    for chunk in &header.chunks {
        println!(
            "    {:<6} offset {:>8}, {:>9} bytes",
            chunk.tag(),
            chunk.offset,
            chunk.size
        );
    }
    let unknown = header.unknown_chunks();
    if unknown.is_empty() {
        println!("    every chunk tag is one the reference's walk recognises");
    } else {
        println!(
            "    unrecognised chunk tag(s): {}",
            unknown
                .iter()
                .map(|c| c.tag())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    match header.setup_packet_consistent() {
        Some(true) => println!(
            "  packet chain     setup packet is immediately followed by the first audio packet, \
             as the reference requires"
        ),
        Some(false) => println!(
            "  packet chain     INCONSISTENT: the first audio packet does not follow the setup \
             packet. Either the vorb offsets are being misread or the file is malformed."
        ),
        None => println!("  packet chain     not applicable to this codec or shape"),
    }
    println!(
        "  convertible      {}",
        if header.convertible_by_reference() {
            "the reference's converter would accept this"
        } else {
            "NO — the reference throws on a header triad or old packet headers"
        }
    );
}

/// The sidecar text written next to a payload. Plain text on purpose: it is a
/// description for a human and for a shell script, not a machine format.
fn sidecar(asset: u64, header: &WemHeader, bytes: &[u8]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# ow-tank-sound sidecar for 0x{asset:016x}\n\
# This file describes a Wwise RIFF container. The audio itself is NOT decoded by\n\
# this tool: the .wem beside this file is the game's own bytes, untouched.\n\n"
    ));
    out.push_str(&format!("asset               0x{asset:016x}\n"));
    out.push_str(&format!("payload bytes       {}\n", bytes.len()));
    out.push_str(&format!("container bytes     {}\n", header.total_len));
    out.push_str(&format!("codec               {}\n", header.codec));
    out.push_str(&format!(
        "codec word          0x{:04X}\n",
        header.codec.word()
    ));
    out.push_str(&format!("channels            {}\n", header.channels));
    out.push_str(&format!("sample rate         {}\n", header.sample_rate));
    out.push_str(&format!("samples per channel {}\n", header.sample_count));
    if let Some(s) = header.duration_seconds() {
        out.push_str(&format!(
            "length seconds      {:.3}   # derived: samples / rate\n",
            s
        ));
    }
    out.push_str(&format!(
        "endianness          {}\n",
        if header.big_endian { "big" } else { "little" }
    ));
    out.push_str(&format!(
        "packet header size  {}\n",
        header.packet_header_size
    ));
    out.push_str(&format!("no granule          {}\n", header.no_granule));
    out.push_str(&format!("mod packets         {}\n", header.mod_packets));
    out.push_str(&format!(
        "header triad        {}\n",
        header.header_triad_present
    ));
    out.push_str(&format!("loop count          {}\n", header.loop_count));
    out.push_str(&format!("loop start          {}\n", header.loop_start));
    out.push_str(&format!("loop end            {}\n", header.loop_end));
    out.push_str(&format!("data chunk offset   {}\n", header.data_offset));
    out.push_str(&format!("data chunk size     {}\n", header.data_size));
    if let Some(rel) = header.setup_packet_offset {
        out.push_str(&format!(
            "setup packet offset {rel}   # relative to the data chunk\n"
        ));
    }
    if let Some(at) = header.setup_packet_at {
        out.push_str(&format!(
            "setup packet at     {at}   # absolute in the payload\n"
        ));
    }
    if let Some(len) = header.setup_packet_len() {
        out.push_str(&format!("setup packet size   {len}\n"));
    }
    if let Some(at) = header.first_audio_packet_at {
        out.push_str(&format!(
            "first audio packet  {at}   # absolute in the payload\n"
        ));
    }
    out.push_str("\n# The honest handoff. This tool does not decode Vorbis or Opus. To get\n");
    out.push_str("# sound out of the .wem, run an external tool over the payload file.\n");
    out.push_str("#\n");
    out.push_str("#   Vorbis (SFX, most voice): ww2ogg turns the Wwise container into Ogg\n");
    out.push_str("#   Vorbis. The reference tooling does the same job in-process, with its\n");
    out.push_str("#   own port of ww2ogg at DataTool/ConvertLogic/WEM/WwiseRIFFVorbis.cs,\n");
    out.push_str("#   and then runs revorb over the result so the stream's granule positions\n");
    out.push_str("#   are correct (DataTool/SaveLogic/Combo.cs:990 and :997).\n");
    out.push_str("#   Command form is ww2ogg's own; check its --help for this version:\n");
    out.push_str("#       ww2ogg <in.wem> -o <out.ogg>\n");
    out.push_str("#\n");
    out.push_str("#   Opus (music): vgmstream reads it directly. The reference shells out\n");
    out.push_str("#   to vgmstream-cli for exactly this (DataTool/SaveLogic/Combo.cs:962).\n");
    out.push_str("#\n");
    out.push_str(&format!(
        "#   NOTE: this container reports the reference's converter would {}accept it.\n",
        if header.convertible_by_reference() {
            ""
        } else {
            "NOT "
        }
    ));
    out
}

fn cmd_wem(args: &[String]) -> std::process::ExitCode {
    // The speech manifest matters here: on the reference install the two Opus
    // music assets live there and the base manifest has none, so the speech
    // index is built up front rather than on failure.
    let Some((data, sources)) = open_with_sources() else {
        return std::process::ExitCode::from(2);
    };
    let Some(guid_text) = args.first() else {
        eprintln!("wem wants a GUID\n\n{USAGE}");
        return std::process::ExitCode::from(2);
    };
    let Ok(asset) = u64::from_str_radix(guid_text.trim_start_matches("0x"), 16) else {
        eprintln!("{guid_text:?} is not a hex GUID");
        return std::process::ExitCode::from(2);
    };

    // Which manifest answered is reported rather than guessed at.
    let origin;

    let header = match sound::read_wem(&data, asset) {
        Ok((header, bytes)) => {
            origin = "base manifest".to_string();
            (header, bytes)
        }
        Err(base_error) => {
            let speech = match sound::open_speech(&data, &sources) {
                Ok(Some(index)) => index,
                Ok(None) => {
                    eprintln!("0x{asset:016x}: {base_error}");
                    eprintln!(
                        "  and this install has no speech manifest, so there is nowhere else to \
                         look."
                    );
                    return std::process::ExitCode::from(1);
                }
                Err(e) => {
                    eprintln!("0x{asset:016x}: {base_error}");
                    eprintln!("  the speech manifest could not be opened: {e}");
                    return std::process::ExitCode::from(1);
                }
            };
            match speech.read_wem(&data, asset) {
                Ok((header, bytes)) => {
                    origin = format!("speech manifest ({})", speech.manifest);
                    (header, bytes)
                }
                Err(_) => {
                    eprintln!("0x{asset:016x}: {base_error}");
                    let in_base = data.registry().get(asset).is_some();
                    if !in_base && speech.get(asset).is_none() {
                        eprintln!(
                            "  no manifest lists it at all, so the GUID is wrong. Run \
                             `ow-tank-sound types` and take a GUID from the census."
                        );
                    } else if data.bytes_of(asset).is_err()
                        && speech.bytes_of(&data, asset).is_err()
                    {
                        eprintln!(
                            "  a manifest lists it but neither has it downloaded. In the registry \
                             but not on disk is normal for sound: Blizzard ships records for \
                             everything and downloads only what you have heard."
                        );
                    } else {
                        eprintln!(
                            "  and it is not a Wwise container in the speech manifest either."
                        );
                    }
                    return std::process::ExitCode::from(1);
                }
            }
        }
    };
    let (header, bytes) = header;

    println!(
        "asset 0x{asset:016x}  type 0x{:03X} ({})  index {}  [{}]",
        guid::type_of(asset),
        types::label(guid::type_of(asset)),
        guid::index_of(asset),
        origin
    );
    print_header(&header);

    let Some(out_dir) = flag_path(args, "--out") else {
        return std::process::ExitCode::SUCCESS;
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("could not create {}: {e}", out_dir.display());
        return std::process::ExitCode::from(1);
    }
    let wem_path = out_dir.join(format!("{asset:016x}.wem"));
    let txt_path = out_dir.join(format!("{asset:016x}.wem.txt"));
    if let Err(e) = std::fs::write(&wem_path, &bytes) {
        eprintln!("could not write {}: {e}", wem_path.display());
        return std::process::ExitCode::from(1);
    }
    let mut note = sidecar(asset, &header, &bytes);
    note.push_str(&format!("# source manifest    {origin}\n"));
    if let Err(e) = std::fs::write(&txt_path, note) {
        eprintln!("could not write {}: {e}", txt_path.display());
        return std::process::ExitCode::from(1);
    }
    println!(
        "\nwrote {} ({} bytes) and {}",
        wem_path.display(),
        bytes.len(),
        txt_path.display()
    );
    println!(
        "The payload is the game's own bytes. This tool did not decode it — see \
         docs/formats/sound.md."
    );
    std::process::ExitCode::SUCCESS
}

fn cmd_payloads(args: &[String]) -> std::process::ExitCode {
    let Some(data) = open() else {
        return std::process::ExitCode::from(2);
    };
    let Some(name) = args.first() else {
        eprintln!("payloads wants a type name\n\n{USAGE}");
        return std::process::ExitCode::from(2);
    };
    let Some(type_id) = types::by_name(name) else {
        eprintln!("{name:?} is not a sound type this tool knows; see the list in --help");
        return std::process::ExitCode::from(2);
    };
    let limit = flag_usize(args, "--limit", 5);

    println!(
        "\nfollowing {} -> payload assets -> Wwise RIFF header, on real data\n",
        types::label(type_id)
    );
    let mut walked = 0usize;
    let mut read = 0usize;
    let mut headers = 0usize;
    let mut failures = 0usize;

    for asset in data.guids_of_type(type_id) {
        if walked >= limit {
            break;
        }
        let mut stu = match data.stu_of(asset) {
            Ok(stu) => stu,
            Err(_) => continue,
        };
        let Some(sound) = sound::read_sound_in(&mut stu, asset) else {
            continue;
        };
        walked += 1;
        read += 1;
        let payloads = sound.payload_assets();
        println!(
            "0x{asset:016x}: {} payload(s), bank 0x{:016x}",
            payloads.len(),
            sound.wwise.as_ref().and_then(|b| b.sound_bank).unwrap_or(0)
        );
        for payload in payloads.iter().take(3) {
            match sound::read_wem(&data, *payload) {
                Ok((header, _)) => {
                    headers += 1;
                    println!(
                        "    payload 0x{payload:016x} type 0x{:03X}: {}",
                        guid::type_of(*payload),
                        header.summary()
                    );
                }
                Err(e) => {
                    failures += 1;
                    println!(
                        "    payload 0x{payload:016x} type 0x{:03X}: {e}",
                        guid::type_of(*payload)
                    );
                }
            }
        }
        if payloads.len() > 3 {
            println!("    ... and {} more payload(s)", payloads.len() - 3);
        }
    }
    println!(
        "\n{} record(s) read, {} payload header(s) parsed, {} payload(s) unreadable",
        read, headers, failures
    );
    if read == 0 {
        println!(
            "nothing to follow: no asset of type 0x{type_id:03X} could be read here. That is \
             itself the finding — the registry lists them, the install has not downloaded them."
        );
    }
    std::process::ExitCode::SUCCESS
}

fn cmd_speech(args: &[String]) -> std::process::ExitCode {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("no Overwatch install found; set OW_INSTALL to point at one");
        return std::process::ExitCode::from(2);
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = match GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    println!(
        "\nthe base manifest(s) {} do NOT include voice audio: GameData::open\n\
         deliberately does not read the speech manifest. Opening it here.\n",
        data.manifests.join(", ")
    );
    let index = match sound::open_speech(&data, &sources) {
        Ok(Some(index)) => index,
        Ok(None) => {
            println!(
                "no speech manifest matched platform Win / region RDEV / locale enUS. \
                 That is a fact about this install, not a read failure — check the \
                 root file for a `*_LenUS_speech_*.cmf` entry."
            );
            return std::process::ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("speech manifest failed: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    println!(
        "speech manifest: {}  ({} record(s))\n",
        index.manifest,
        index.assets.len()
    );
    let histogram = index.type_histogram();
    println!("types held:");
    for (type_id, count) in &histogram {
        let mark = if sound::types::ALL.contains(type_id) {
            "sound layer"
        } else {
            "not in the sound layer's list"
        };
        println!("  0x{type_id:03X}  {count:>7}  {mark}");
    }

    // The join from gameplay to voice: does a hero's entity actually name a voice
    // set, and does that GUID land in the speech manifest? This is the one place
    // the two halves of this layer meet, so it is measured rather than assumed.
    println!("\nhero entity -> voice set:");
    let mut heroes = data.guids_of_type(guid::types::HERO);
    heroes.sort_unstable();
    let mut component_found = 0usize;
    let mut in_speech = 0usize;
    let mut entity_read = 0usize;
    let mut shown = 0usize;
    for hero in heroes.iter().take(40) {
        let index_hint = guid::index_of(*hero);
        let Ok(parts) = ow_tank::entity::hero_parts(&data, index_hint) else {
            continue;
        };
        if parts.entity == 0 {
            continue;
        }
        entity_read += 1;
        let Ok(Some(voice_set)) = sound::voice_set_ref_of_entity(&data, parts.entity) else {
            continue;
        };
        component_found += 1;
        if index.get(voice_set).is_some() {
            in_speech += 1;
        }
        if shown < 6 {
            shown += 1;
            println!(
                "  hero {index_hint}: entity 0x{:016x} -> voice set 0x{voice_set:016x} {}",
                parts.entity,
                if index.get(voice_set).is_some() {
                    "(in the speech manifest)"
                } else {
                    "(NOT in the speech manifest)"
                }
            );
        }
    }
    println!(
        "  {entity_read} hero entit(y/ies) read, {component_found} named a voice set, \
         {in_speech} of those landed in {}",
        index.manifest
    );
    if entity_read == 0 {
        println!("  no hero entity could be read on this install, so the join was not exercised.");
    }

    // For the sound types the layer knows, try a handful of payloads.
    let cap = flag_usize(args, "--cap", 5);
    println!("\nWwise headers from the sound payload types:");
    for type_id in sound::types::WEM_PAYLOAD {
        let guids = index.guids_of_type(*type_id);
        if guids.is_empty() {
            println!(
                "  0x{type_id:03X} {}: no assets",
                sound::types::label(*type_id)
            );
            continue;
        }
        let step = (guids.len() / cap.max(1)).max(1);
        let sample: Vec<u64> = guids.iter().copied().step_by(step).take(cap).collect();
        let mut ok = 0usize;
        let mut bad = 0usize;
        let mut missing = 0usize;
        for guid in sample {
            match index.read_wem(&data, guid) {
                Ok((header, _)) => {
                    ok += 1;
                    println!(
                        "  0x{guid:016x}  type 0x{type_id:03X}  {} bytes  {}",
                        index.get(guid).map(|r| r.size).unwrap_or(0),
                        header.summary()
                    );
                }
                Err(e) => {
                    if index.bytes_of(&data, guid).is_err() {
                        missing += 1;
                    } else {
                        bad += 1;
                        println!("  0x{guid:016x}  type 0x{type_id:03X}  unreadable: {e}");
                    }
                }
            }
        }
        println!(
            "    -> {} header(s) parsed, {} present but unreadable, {} not downloaded",
            ok, bad, missing
        );
    }
    std::process::ExitCode::SUCCESS
}

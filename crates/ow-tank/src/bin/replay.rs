//! Dump a highlight replay: the `prp` block and the `phl` container around it.
//!
//! ```text
//! ow-tank-replay <file|dir>...     # a Highlights folder, or one file in it
//! ow-tank-replay --find            # list where highlights are looked for
//! ```
//!
//! Replays are not in the game install — they live in the user's application
//! data folder under `Blizzard Entertainment\Overwatch\<account>\Highlights\`.
//! This tool reads whatever path it is given and writes nothing. Set
//! `OW_HIGHLIGHTS` to point it at a different Highlights folder.
//!
//! It prints the container's verified header, the replay's verified prefix, and
//! then what the reference's declared layout makes of the rest. That last part
//! does not line up with the files on this machine and usually comes back as an
//! error; the report says so rather than printing a roster it cannot justify.
//! See `docs/formats/replay.md`.

use std::path::{Path, PathBuf};

use ow_tank::guid;
use ow_tank::replay::{self, Container, Error, Replay, ZSTD_MAGIC};

/// Where the game keeps highlight replays: `%LOCALAPPDATA%` on this platform.
fn default_roots() -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    vec![Path::new(&local)
        .join("Blizzard Entertainment")
        .join("Overwatch")]
}

/// Every file to look at: the given paths, plus every file under any `Highlights`
/// directory beneath them.
fn collect(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_file() {
            out.push(path.clone());
            continue;
        }
        if !path.is_dir() {
            eprintln!("{}: not a file or directory", path.display());
            continue;
        }
        let highlights = path.join("Highlights");
        let search = if highlights.is_dir() {
            highlights
        } else {
            path.clone()
        };
        let mut found: Vec<PathBuf> = match std::fs::read_dir(&search) {
            Ok(entries) => entries.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(e) => {
                eprintln!("{}: {e}", search.display());
                continue;
            }
        };
        found.retain(|p| p.is_file());
        found.sort();
        if found.is_empty() {
            eprintln!("{}: no files", search.display());
        }
        out.extend(found);
    }
    out
}

fn describe_guid(label: &str, value: u64) -> String {
    if value == 0 {
        return format!("{label}: none");
    }
    format!(
        "{label}: 0x{value:016x} (type 0x{:03x}, index {})",
        guid::type_of(value),
        guid::index_of(value)
    )
}

fn dump_container(bytes: &[u8], name: &str) {
    match Container::read(bytes) {
        Ok(c) => {
            println!("container {name}: phl");
            println!("  format version:      {}", c.format_version);
            println!("  checksum:            {}", c.checksum);
            println!(
                "  data length:         {} (file is {} bytes, so the header prefix is {})",
                c.data_length,
                bytes.len(),
                bytes.len() as i64 - c.data_length as i64
            );
            println!("  replay version:      {}", c.replay_version);
            println!("  build number:        {}", c.build_number);
            println!("  player id:           {}", c.player_id);
            println!("  flags:               {:#010x}", c.flags);
            println!("  {}", describe_guid("map", c.map));
            println!("  {}", describe_guid("game mode", c.game_mode));
        }
        Err(e) => println!("container {name}: not a phl ({e})"),
    }
}

fn dump_replay(block: &[u8]) {
    let header = match replay::Header::read(block) {
        Ok(h) => h,
        Err(e) => {
            println!("  replay header: {e}");
            return;
        }
    };
    println!(
        "  replay header (verified prefix, {}+ bytes):",
        replay::PREFIX_LEN
    );
    println!("    format version:  {}", header.format_version);
    println!("    build number:    {}", header.build_number);
    println!("    {}", describe_guid("map", header.map));
    println!("    {}", describe_guid("game mode", header.game_mode));

    // Where the payload really starts, from the bytes rather than the layout.
    let frame_at = replay::first_zstd_frame(block);
    match frame_at {
        Some(at) if block[at..].starts_with(&ZSTD_MAGIC) => println!(
            "    payload:         Zstd frame at +{at}, {} compressed bytes to the end of \
             the block",
            block.len() - at
        ),
        Some(at) => println!("    payload:         magic at +{at} but not a Zstd frame"),
        None => println!("    payload:         no Zstd frame magic found in the block"),
    }

    match Replay::parse(block) {
        Ok(r) => {
            println!("  declared layout (as the reference reader declares it):");
            println!("    unknown1:        {:#04x}", r.unknown1);
            println!("    unknown2:        {}", r.unknown2);
            println!("    unknown3:        {}", r.unknown3);
            println!("    map checksum:    {}", r.map_checksum);
            println!("    params length:   {}", r.params_block_length);
            println!(
                "    params:          frames {}..{}, duration {} ms, window {}..{} ms, {} hero(s)",
                r.params.start_frame,
                r.params.end_frame,
                r.params.expected_duration_ms,
                r.params.start_ms,
                r.params.end_ms,
                r.params.heroes.len()
            );
            for hero in &r.params.heroes {
                println!("      {}", describe_guid("hero", hero.hero));
                println!(
                    "        skin {}  weapon skin {}  sprays {:?}  voice lines {:?}  emotes {:?}",
                    hero.skin_id,
                    hero.weapon_skin_id,
                    hero.spray_ids,
                    hero.voice_line_ids,
                    hero.emote_ids
                );
            }
            if let Some(info) = &r.highlight_info {
                println!(
                    "    highlight:       kind {} by {:?}, team {}, at {:?}",
                    info.kind, info.player_name, info.team_index, info.position
                );
                println!("                    {}", describe_guid("hero", info.hero));
                println!("                    {}", info.uuid);
            }
            println!(
                "    payload at +{}:  {} compressed bytes, uncompressed length {}",
                r.payload_offset,
                r.payload.compressed_len(),
                match r.payload.uncompressed_len() {
                    Some(n) => n.to_string(),
                    None => "not recorded by the writer".to_string(),
                }
            );
            // The check that says whether the declared layout is right: the
            // payload has to begin where the Zstd frame does.
            if frame_at != Some(r.payload_offset) {
                println!(
                    "    NOTE: the declared layout puts the payload at +{} but the Zstd \
                     frame is at +{:?}; the fields above are not trustworthy. See \
                     docs/formats/replay.md.",
                    r.payload_offset, frame_at
                );
            }
            match r.payload.decode() {
                Err(Error::PayloadNotDecoded { .. }) => {
                    println!("    payload decode:  refused (no Zstd decoder in this crate)")
                }
                other => println!("    payload decode:  {other:?}"),
            }
        }
        Err(e) => println!(
            "  declared layout: does not parse these bytes ({e}) — the reference's field \
             order does not match this file. The verified prefix above still holds. See \
             docs/formats/replay.md."
        ),
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--find") || args.is_empty() {
        let roots = match std::env::var_os("OW_HIGHLIGHTS") {
            Some(p) => vec![PathBuf::from(p)],
            None => default_roots(),
        };
        println!("highlight replays are not in the game install; look for them under:");
        for root in roots {
            if root.is_dir() {
                let mut accounts: Vec<PathBuf> = match std::fs::read_dir(&root) {
                    Ok(entries) => entries.filter_map(|e| e.ok().map(|e| e.path())).collect(),
                    Err(_) => Vec::new(),
                };
                accounts.retain(|p| p.join("Highlights").is_dir());
                accounts.sort();
                if accounts.is_empty() {
                    println!("  {}: no account with a Highlights folder", root.display());
                }
                for account in accounts {
                    println!("  {}", account.join("Highlights").display());
                }
            } else {
                println!("  {}: not present", root.display());
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let files = collect(&paths);
    if files.is_empty() {
        eprintln!("nothing to read");
        return std::process::ExitCode::from(2);
    }

    let mut read = 0usize;
    let mut blocks = 0usize;
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("{}: unreadable", path.display());
            continue;
        };
        read += 1;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        println!("{}  ({} bytes)", path.display(), bytes.len());

        if bytes.starts_with(&replay::CONTAINER_MAGIC) {
            dump_container(&bytes, &name);
        }
        match replay::find_replay(&bytes) {
            Ok(block) => {
                blocks += 1;
                println!(
                    "  replay block:     at +{}, {} bytes (length prefix agrees)",
                    block.offset, block.length
                );
                dump_replay(&bytes[block.offset..block.offset + block.length]);
            }
            Err(e) => println!("  replay block:     none ({e})"),
        }
        println!();
    }
    eprintln!("read {read} file(s), found {blocks} replay block(s)");
    std::process::ExitCode::SUCCESS
}

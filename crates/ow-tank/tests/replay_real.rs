//! The `prp` replay reader against real files: a synthetic round-trip first, then
//! a sweep of whatever highlight replays the operator actually has.
//!
//! Replays are **not** in the game install. There is no asset type id for them
//! — OWLib's `AssetRepoType` enum has no replay member — and the game writes
//! them into the user's application data folder:
//!
//! ```text
//! %LOCALAPPDATA%\Blizzard Entertainment\Overwatch\<account>\Highlights\<uuid>
//! ```
//!
//! (`DataTool/ToolLogic/List/Misc/ListHighlights.cs:74-79`, which inserts a
//! `PTR\` level on the test server). `OW_HIGHLIGHTS` overrides the search. When
//! no such folder exists the sweep prints that and returns, asserting nothing.
//!
//! Run the sweep with output:
//!
//! ```text
//! cargo test -p ow-tank --test replay_real -- --nocapture
//! ```

use std::path::{Path, PathBuf};

use ow_tank::replay::{
    self, Container, Error, Replay, CONTAINER_PREFIX_LEN, MAGIC, PREFIX_LEN, ZSTD_MAGIC,
};

// ---------------------------------------------------------------------------
// Synthetic round-trip: the parser is proven against bytes built at the offsets
// it reads, so an offset that drifts fails here rather than quietly agreeing.
// ---------------------------------------------------------------------------

/// Builds a `prp` block at the exact offsets `Replay::parse` reads.
fn synthetic_block(with_highlight: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.push(11); // +0x03 format version
    out.extend_from_slice(&154_088u32.to_le_bytes()); // +0x04 build
    out.extend_from_slice(&0x0800_0000_0000_069eu64.to_le_bytes()); // +0x08 map
    out.extend_from_slice(&0x0230_0000_0000_0172u64.to_le_bytes()); // +0x10 game mode
    out.push(if with_highlight {
        replay::highlight::HAS_HIGHLIGHT_INFO
    } else {
        0
    }); // +0x18
    out.extend_from_slice(&7u32.to_le_bytes()); // +0x19
    out.extend_from_slice(&9u32.to_le_bytes()); // +0x1D
    out.extend_from_slice(&[0xAB; 32]); // +0x21 map checksum
    let length_at = out.len();
    out.extend_from_slice(&0i32.to_le_bytes()); // +0x41 patched below

    let params_at = out.len();
    out.extend_from_slice(&16u32.to_le_bytes()); // start frame
    out.extend_from_slice(&76u32.to_le_bytes()); // end frame
    out.extend_from_slice(&18_000u64.to_le_bytes()); // expected duration, ms
    out.extend_from_slice(&1_200u64.to_le_bytes()); // start, ms
    out.extend_from_slice(&19_200u64.to_le_bytes()); // end, ms
    out.extend_from_slice(&2i32.to_le_bytes()); // hero count
    for (hero_index, sprays) in [(2u64, &[0x11u32, 0x12][..]), (6, &[][..])] {
        out.extend_from_slice(&100u32.to_le_bytes()); // skin id
        out.extend_from_slice(&200u32.to_le_bytes()); // weapon skin id
        out.extend_from_slice(&300u32.to_le_bytes()); // potg animation
        out.extend_from_slice(&(sprays.len() as i32).to_le_bytes());
        for v in sprays {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for _ in 0..2 {
            out.extend_from_slice(&0i32.to_le_bytes()); // voice lines, emotes
        }
        out.extend_from_slice(&400u32.to_le_bytes()); // announcer
        out.extend_from_slice(&(0x02E0_0000_0000_0000u64 + hero_index).to_le_bytes());
    }
    let params_len = (out.len() - params_at) as i32;
    out[length_at..length_at + 4].copy_from_slice(&params_len.to_le_bytes());

    if with_highlight {
        let mut info = Vec::new();
        info.extend_from_slice(b"Rye");
        info.push(0); // name terminator
        info.push(0); // unknown byte
        info.push(replay::highlight::KIND_MANUAL);
        info.extend_from_slice(&1u32.to_le_bytes());
        info.extend_from_slice(&2u32.to_le_bytes());
        info.extend_from_slice(&3.5f32.to_le_bytes());
        info.extend_from_slice(&4.5f32.to_le_bytes());
        info.extend_from_slice(&5u32.to_le_bytes());
        info.extend_from_slice(&1i32.to_le_bytes());
        for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
            info.extend_from_slice(&v.to_le_bytes());
        }
        for v in [
            0x02E0_0000_0000_0011u64,
            0x0250_0000_0000_0012,
            0x0250_0000_0000_0013,
            0x0790_0000_0000_0014,
            0x0790_0000_0000_0015,
            0x0250_0000_0000_0016,
            0x0ABC_0000_0000_0017,
        ] {
            info.extend_from_slice(&v.to_le_bytes());
        }
        info.extend_from_slice(&123_456u64.to_le_bytes());
        info.extend_from_slice(&std::array::from_fn::<u8, 16, _>(|i| i as u8));
        out.extend_from_slice(&(info.len() as i32).to_le_bytes());
        out.extend_from_slice(&info);
    }

    // A Zstd frame with no content-size field, which is what every real replay
    // measured carries.
    out.extend_from_slice(&ZSTD_MAGIC);
    out.push(0x00);
    out.push(0x40);
    out.extend_from_slice(&[0xCD; 16]);
    out
}

#[test]
fn synthetic_replay_round_trips() {
    // Without a highlight description.
    let block = synthetic_block(false);
    let replay = Replay::parse(&block).expect("synthetic block parses");

    assert_eq!(replay.header.format_version, 11);
    assert_eq!(replay.header.build_number, 154_088);
    assert_eq!(replay.header.map, 0x0800_0000_0000_069e);
    assert_eq!(replay.header.game_mode, 0x0230_0000_0000_0172);
    // The declared length must agree with what the reader walked: five scalars, the
    // hero count, then 36 bytes per hero plus 4 per cosmetic.
    assert_eq!(
        replay.params_block_length,
        4 * 2 + 8 * 3 + 4 + (36 + 8) + 36
    );
    assert_eq!(replay.params.start_frame, 16);
    assert_eq!(replay.params.end_frame, 76);
    assert_eq!(replay.params.expected_duration_ms, 18_000);
    assert_eq!(replay.params.heroes.len(), 2);
    assert_eq!(replay.params.heroes[0].hero, 0x02E0_0000_0000_0002);
    assert_eq!(replay.params.heroes[0].spray_ids, vec![0x11, 0x12]);
    assert!(replay.highlight_info.is_none());
    // The payload starts exactly where the Zstd frame does, which is the check
    // that says the declared layout was walked correctly.
    assert_eq!(
        replay.payload_offset,
        replay::first_zstd_frame(&block).expect("frame present")
    );
    assert!(replay.payload.has_frame_magic());
    assert_eq!(replay.payload.compressed_len(), 22);
    assert_eq!(replay.payload.uncompressed_len(), None);

    // With one.
    let block = synthetic_block(true);
    let replay = Replay::parse(&block).expect("synthetic block parses");
    let info = replay.highlight_info.expect("highlight info present");
    assert_eq!(info.player_name, "Rye");
    assert_eq!(info.kind, replay::highlight::KIND_MANUAL);
    assert_eq!(info.team_index, 1);
    assert_eq!(info.hero, 0x02E0_0000_0000_0011);
    assert_eq!(info.timestamp, 123_456);
    assert_eq!(
        info.uuid.to_hyphenated(),
        "03020100-0504-0706-0809-0a0b0c0d0e0f"
    );
    assert_eq!(
        replay.payload_offset,
        replay::first_zstd_frame(&block).expect("frame present")
    );

    // The checksum is exposed, not derived: no SHA-2 in this crate.
    assert_eq!(replay.map_checksum.to_string().len(), 64);
    assert!(replay.map_checksum.to_string().starts_with("ABABABAB"));

    // The payload reports that it is not decoded, with its real lengths.
    match replay.payload.decode() {
        Err(Error::PayloadNotDecoded {
            compressed_len,
            declared_uncompressed_len,
        }) => {
            assert_eq!(compressed_len, 22);
            assert_eq!(declared_uncompressed_len, None);
        }
        other => panic!("decode returned {other:?}, expected PayloadNotDecoded"),
    }
}

// ---------------------------------------------------------------------------
// The real sweep
// ---------------------------------------------------------------------------

/// Where highlights are looked for: `OW_HIGHLIGHTS` if set, otherwise the
/// platform's `%LOCALAPPDATA%\Blizzard Entertainment\Overwatch`.
fn highlight_roots() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("OW_HIGHLIGHTS") {
        return vec![PathBuf::from(p)];
    }
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    vec![Path::new(&local)
        .join("Blizzard Entertainment")
        .join("Overwatch")]
}

/// Every highlight file under the roots, or `None` when there is nothing.
fn highlight_files() -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    for root in highlight_roots() {
        // The root may be a Highlights folder itself or the Overwatch folder.
        let direct = root.join("Highlights");
        let search = if direct.is_dir() {
            direct
        } else {
            root.clone()
        };
        let Ok(entries) = std::fs::read_dir(&search) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(inner) = std::fs::read_dir(path.join("Highlights")) {
                    out.extend(inner.filter_map(|e| e.ok().map(|e| e.path())));
                }
            } else {
                out.push(path);
            }
        }
    }
    out.retain(|p| p.is_file());
    out.sort();
    (!out.is_empty()).then_some(out)
}

#[test]
fn real_replays() {
    let Some(files) = highlight_files() else {
        eprintln!(
            "note: no highlight replays found (looked under {:?}). Replays are not in \
             the game install, so nothing to assert; the synthetic round-trip above is \
             what covers the parser.",
            highlight_roots()
        );
        return;
    };
    eprintln!("highlight files found: {}", files.len());

    let file_count = files.len();
    let mut blocks = 0usize;
    let mut parsed = 0usize;
    let mut frames = 0usize;
    let mut declared_sizes = 0usize;
    let mut layout_agreed = 0usize;
    let mut layout_errors: Vec<String> = Vec::new();

    for path in &files {
        let bytes = std::fs::read(path).expect("highlight file is readable");
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        // ------------------------------------------------------------------
        // The container's verified prefix
        // ------------------------------------------------------------------
        let mut container: Option<Container> = None;
        if bytes.starts_with(&replay::CONTAINER_MAGIC) {
            let c = Container::read(&bytes).expect("a phl prefix of the declared size parses");
            assert_eq!(
                bytes.len() as i64 - c.data_length as i64,
                40,
                "{name}: data_length does not leave a 40-byte prefix, so the phl header \
                 layout is not what this reader declares"
            );
            assert!(c.build_number > 0, "{name}: no build number");
            assert!(c.player_id > 0, "{name}: no player id");
            assert_eq!(c.format_version, 5, "{name}: unexpected phl format version");
            container = Some(c);
        }

        // ------------------------------------------------------------------
        // The block, and its verified prefix
        // ------------------------------------------------------------------
        let found = match replay::find_replay(&bytes) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{name}: no replay block ({e})");
                continue;
            }
        };
        blocks += 1;
        assert_eq!(
            found.length,
            bytes.len() - found.offset,
            "{name}: the length prefix must equal the bytes that follow it"
        );
        let block = &bytes[found.offset..found.offset + found.length];

        let header = replay::Header::read(block).expect("the verified prefix parses");
        assert!(
            found.offset >= 4,
            "{name}: the block must be preceded by its own length"
        );
        assert_eq!(
            block[..3],
            MAGIC,
            "{name}: found_replay located something that is not prp"
        );
        assert_eq!(
            replay::PREFIX_LEN,
            24,
            "the verified prefix length is a contract with this test"
        );

        if let Some(c) = container {
            assert_eq!(
                header.map, c.map,
                "{name}: the replay's map must be the container's map, which is what pins \
                 the map offset at +0x08"
            );
            assert_eq!(
                header.game_mode, c.game_mode,
                "{name}: the replay's game mode must be the container's, which is what \
                 pins the game-mode offset at +0x10"
            );
            eprintln!(
                "{name}: build {} (container build {}) map 0x{:016x} game mode 0x{:016x}",
                header.build_number, c.build_number, header.map, header.game_mode
            );
        }

        // ------------------------------------------------------------------
        // The payload: present, Zstd, and not decoded
        // ------------------------------------------------------------------
        let frame_at = replay::first_zstd_frame(block);
        assert!(
            frame_at.is_some(),
            "{name}: no Zstd frame inside the replay block"
        );
        let frame_at = frame_at.unwrap();
        assert!(
            block[frame_at..].starts_with(&ZSTD_MAGIC),
            "{name}: first_zstd_frame did not land on the magic"
        );
        frames += 1;
        let frame_len = block.len() - frame_at;
        let probe = ZstdProbe::read(&block[frame_at..]);
        if probe.content_size.is_some() {
            declared_sizes += 1;
        }
        eprintln!(
            "{name}: header {PREFIX_LEN} B, payload {frame_len} B at +{frame_at}, \
             frame content size {}, decode: not attempted (no Zstd decoder)",
            match probe.content_size {
                Some(n) => n.to_string(),
                None => "not recorded".to_string(),
            }
        );

        // ------------------------------------------------------------------
        // The declared layout, which is expected *not* to line up
        // ------------------------------------------------------------------
        match Replay::parse(block) {
            Ok(replay) => {
                parsed += 1;
                if replay.payload_offset == frame_at {
                    layout_agreed += 1;
                }
            }
            Err(e) => layout_errors.push(format!("{name}: {e}")),
        }
    }

    assert!(blocks > 0, "no highlight file contained a prp block");
    assert_eq!(frames, blocks, "every block must carry a Zstd payload");
    const _: () = assert!(
        CONTAINER_PREFIX_LEN == 108,
        "the declared container prefix length is a contract with this test"
    );

    eprintln!(
        "summary: {blocks} replay block(s) in {file_count} file(s); {parsed} parsed by the \
         reference's declared layout; {layout_agreed} of those put the payload exactly \
         where the Zstd frame is; {declared_sizes} frame(s) declared a content size"
    );
    if !layout_errors.is_empty() {
        eprintln!(
            "  the declared layout does not describe these files ({} of {blocks}); first: {}",
            layout_errors.len(),
            layout_errors[0]
        );
        eprintln!(
            "  this is the recorded finding, not a regression: see docs/formats/replay.md. \
             The verified prefix and the payload are unaffected."
        );
    }
}

/// Reads the Zstd frame header's declared content size without decompressing.
///
/// The frame header is a 4-byte magic, a descriptor byte whose top two bits give
/// the width of the content-size field and whose bit 5 says whether the window
/// descriptor is present, an optional window byte, an optional dictionary id,
/// then the content size.
struct ZstdProbe {
    content_size: Option<u64>,
}

impl ZstdProbe {
    fn read(frame: &[u8]) -> ZstdProbe {
        let mut content_size = None;
        if frame.len() >= 6 && frame[..4] == ZSTD_MAGIC {
            let descriptor = frame[4];
            let flag = descriptor >> 6;
            let single_segment = (descriptor >> 5) & 1 == 1;
            let dictionary_id_flag = descriptor & 0x3;
            let mut at = 5usize;
            if !single_segment {
                at += 1;
            }
            at += [0usize, 1, 2, 4][dictionary_id_flag as usize];
            let width = match flag {
                0 => {
                    if single_segment {
                        1
                    } else {
                        0
                    }
                }
                1 => 2,
                2 => 4,
                _ => 8,
            };
            if width > 0 && frame.len() >= at + width {
                let mut value = 0u64;
                for (i, b) in frame[at..at + width].iter().enumerate() {
                    value |= (*b as u64) << (8 * i);
                }
                if width == 2 {
                    value += 256;
                }
                content_size = Some(value);
            }
        }
        ZstdProbe { content_size }
    }
}

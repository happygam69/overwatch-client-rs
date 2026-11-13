//! The Wwise sound bank container (`BKHD`) against the real install.
//!
//! proof-covers: 0x1428fc300 0x1428fa3a0
//!
//! Every registered type-`0x043` asset is opened and parsed with
//! [`ow_tank::wwise_bank`]. The reader follows the client's own header parse
//! (`ReadBankHeader` at `0x1428fc300`) and section dispatcher (`0x1428fa3a0`);
//! this test is what makes it a measurement rather than a transcription:
//!
//! 1. every readable bank parses, and every version lands inside the client's
//!    `0x76..=0x91` window - the strongest evidence that the header fields sit
//!    where the reader puts them, because a wrong offset or the XOR key would
//!    move the version outside the window;
//! 2. the sections tile the file exactly, with no gap and no trailing byte
//!    (`parse` refuses otherwise, so this is asserted by the parse succeeding);
//! 3. every `DIDX` record lies inside the `DATA` payload and its offset is a
//!    multiple of the header's alignment - which also settles *which* header
//!    `u16` the client's `DATA` handler reads, since the second candidate is
//!    zero on this install and cannot be a modulus;
//! 4. the `BKHD` payload is at least the `0x28` bytes the reader consumes for
//!    versions `0x8f` and above, and one bank declares more than that, which
//!    the client reaches by skipping the remainder.
//!
//! Skipped when no install is present, like `tests/sound_real.rs`. Run with:
//!
//! ```text
//! cargo test --release -p ow-tank --test bank_real -- --nocapture
//! ```

use std::collections::BTreeMap;

use ow_tank::wwise_bank::{self, Bank, Section};
use ow_tank::{GameData, ManifestSelector};

const SOUND_BANK: u16 = 0x043;

/// Open the install, or `None` when there is none to open.
fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    GameData::open(&root, &ManifestSelector::default(), &sources).ok()
}

/// The sections a bank actually carries, as a printable signature.
fn section_names(bank: &Bank) -> String {
    bank.sections
        .iter()
        .map(Section::name)
        .collect::<Vec<_>>()
        .join("+")
}

#[test]
fn every_readable_sound_bank_parses() {
    let Some(data) = open() else {
        eprintln!("no install here; nothing to measure");
        return;
    };
    let guids = data.guids_of_type(SOUND_BANK);
    if guids.is_empty() {
        eprintln!("no type-{SOUND_BANK:03X} assets in this install");
        return;
    }

    let mut parsed = 0usize;
    let mut missing = 0usize;
    let mut failures: Vec<(u64, String)> = Vec::new();
    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();
    let mut signatures: BTreeMap<String, usize> = BTreeMap::new();
    let mut with_media = 0usize;
    let mut media_total = 0usize;
    let mut alignment_values: BTreeMap<u16, usize> = BTreeMap::new();
    let mut misaligned = 0usize;
    let mut outside_data = 0usize;
    let mut header_sizes: BTreeMap<u32, usize> = BTreeMap::new();
    let mut data_bytes = 0u64;

    for asset in guids {
        let bytes = match data.bytes_of(asset) {
            Ok(b) => b,
            Err(_) => {
                missing += 1;
                continue;
            }
        };
        let bank = match wwise_bank::parse(&bytes) {
            Ok(b) => b,
            Err(e) => {
                failures.push((asset, e.to_string()));
                continue;
            }
        };
        parsed += 1;
        *versions.entry(bank.header.version).or_default() += 1;
        *signatures.entry(section_names(&bank)).or_default() += 1;
        *header_sizes.entry(bank.header.section_size).or_default() += 1;
        let align = bank.header.data_alignment();
        *alignment_values.entry(align).or_default() += 1;
        if let Some(s) = bank.section(wwise_bank::SECTION_DATA) {
            data_bytes += s.size as u64;
        }
        if !bank.media.is_empty() {
            with_media += 1;
            media_total += bank.media.len();
            let data_size = bank
                .section(wwise_bank::SECTION_DATA)
                .map(|s| s.size as u64)
                .unwrap_or(0);
            for m in &bank.media {
                if align != 0 && m.offset % align as u32 != 0 {
                    misaligned += 1;
                }
                if m.offset as u64 + m.size as u64 > data_size {
                    outside_data += 1;
                }
            }
        }
    }

    println!("\nsound banks (type 0x043) on this install");
    println!(
        "  registered {:<6} opened {parsed:<6} not on disk {missing}",
        guids_len(&data)
    );
    println!("  versions   {versions:?}");
    println!("  BKHD size  {header_sizes:?}");
    println!("  section signatures {signatures:?}");
    println!("  alignment  {alignment_values:?}");
    println!(
        "  banks with DIDX {with_media}, media records {media_total}, DATA bytes {data_bytes}"
    );
    println!("  misaligned DIDX offsets {misaligned}, outside DATA {outside_data}");
    for (asset, why) in failures.iter().take(8) {
        println!("  FAIL 0x{asset:016x}: {why}");
    }

    assert!(
        failures.is_empty(),
        "{} readable sound bank(s) did not parse",
        failures.len()
    );

    // The client's window (`0x1428fc41a..0x1428fc434`). A wrong header offset,
    // a wrong version width, or a non-zero XOR key all move the version out of
    // this range.
    for version in versions.keys() {
        assert!(
            *version > wwise_bank::VERSION_MIN_EXCLUSIVE
                && *version < wwise_bank::VERSION_MAX_EXCLUSIVE,
            "version 0x{version:X} is outside the client's window 0x76..=0x91"
        );
    }

    // Every bank on this install is a version-`0x8f`-or-later bank, so the
    // client reads the extended `0x28`-byte header and then *skips* whatever a
    // larger `BKHD` payload declares. One bank declares `0x30`; it parses
    // because the skip covers the extra 8 bytes, and the reader must not
    // assume the payload ends where the header does.
    for size in header_sizes.keys() {
        assert!(
            *size >= 0x28,
            "a bank declares a BKHD payload of 0x{size:X}, shorter than the 0x28 the reader reads"
        );
    }

    if media_total > 0 {
        assert_eq!(
            misaligned, 0,
            "{misaligned} DIDX offset(s) are not a multiple of the header alignment"
        );
        assert_eq!(
            outside_data, 0,
            "{outside_data} DIDX record(s) run past the DATA payload"
        );
    }
}

/// The registry's asset count for the type, for the printout.
fn guids_len(data: &GameData) -> usize {
    data.guids_of_type(SOUND_BANK).len()
}

#[test]
fn a_non_bank_asset_is_not_mistaken_for_one() {
    // Cheap guard on the reader, independent of the install: the magic check
    // must reject anything that merely starts with `BKH`.
    for bad in [&b"BKHX"[..], b"RIFF", b"OWSTU"] {
        assert!(wwise_bank::parse(bad).is_err());
    }
}

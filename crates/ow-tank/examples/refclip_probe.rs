//! Inspect a 0x00020400-family ("first-person") clip through the
//! reference-table decoder, and test its id list against the FP and
//! character rigs.
//!
//! usage: cargo run -p ow-tank --example refclip_probe -- [clip hex]

use ow_tank::{guid, GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let c15a = 0x0a00_0000_0000_c15a_u64;
    let guid_arg = std::env::args()
        .nth(1)
        .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap())
        .unwrap_or(c15a);
    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let bytes = match data.bytes_of(guid_arg) {
        Ok(b) => b,
        Err(e) => {
            println!("{guid_arg:#018x}: {e:?}");
            return std::process::ExitCode::SUCCESS;
        }
    };
    let clip = match ow_tank::clip::Clip::parse(&bytes) {
        Ok(c) => c,
        Err(e) => {
            println!("{guid_arg:#018x}: not a clip: {e:?}");
            return std::process::ExitCode::SUCCESS;
        }
    };
    println!(
        "{guid_arg:#018x}: family {:#010x}, {} frames @ {} fps, {} bytes, sections {:?}",
        clip.header.family,
        clip.header.frames,
        clip.header.fps,
        bytes.len(),
        clip.sections
    );
    println!(
        "first_section: {} words, head {:?}",
        clip.first_section.len(),
        &clip.first_section[..clip.first_section.len().min(12)]
    );
    let ids: Vec<u32> = (0..)
        .map_while(|i| {
            let o = 0x80 + i * 4;
            (o + 4 <= bytes.len()).then(|| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()))
        })
        .collect();
    println!("id-list head: {:?} ({} words)", &ids[..ids.len().min(12)], ids.len());
    let tables = ow_tank::clip::decode_reference_tables(&bytes);
    match &tables {
        Some(t) => {
            let with_tracks = t.iter().filter(|x| !x.is_empty()).count();
            println!(
                "reference tables: {} entries, {} with tracks",
                t.len(),
                with_tracks
            );
            for (i, tracks) in t.iter().enumerate().take(3) {
                let kinds: Vec<String> = tracks
                    .iter()
                    .map(|tr| format!("{:?}:{}", tr.track, tr.keys.len()))
                    .collect();
                println!("  table {i}: {kinds:?}");
            }
        }
        None => println!("reference tables: none"),
    }
    // id coverage against the two rigs
    let parts = ow_tank::entity::hero_parts(&data, 3).expect("hero");
    for (label, model) in [("fp", parts.view_model), ("char", parts.model)] {
        if let Ok(mb) = data.bytes_of(model) {
            if let Ok(skel) = ow_tank::skeleton::parse_model(&mb) {
                let cover = ids.iter().filter(|id| skel.ids.contains(id)).count();
                println!("{label} rig: {}/{} id-list ids present", cover, ids.len());
            }
        }
    }
    println!(
        "clip id type word: {:?}",
        ids.iter().map(|v| v >> 24).collect::<std::collections::HashSet<_>>()
    );
    let _ = guid::type_of(guid_arg);
    std::process::ExitCode::SUCCESS
}

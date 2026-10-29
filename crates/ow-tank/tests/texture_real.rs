//! Type-`004` texture assets from the operator's installation.
//!
//! The unit tests in `ow_tank::texture` pin six real headers byte for byte.
//! This test is the other direction: it reads the live install through the
//! client's bundle-aware open (`GameData::open_asset`, not the store-only
//! `bytes_of`) and checks that every sampled asset obeys the invariants the
//! read code and the install data imply:
//!
//! * `payload_count == popcount(chunk_mask)` (an install relation, not a proven
//!   per-bit meaning — see the `chunk_mask` note in `docs/formats/materials.md`);
//! * `base_mips <= mip_count`;
//! * `total_size == cooked_size(...)`, the **on-disk block-ceil sum**, not the
//!   runtime `size_walk`; the two differ by 20–84 bytes for 5 non-power-of-two
//!   block textures in this sample;
//! * an embedded chunk's levels end at the smallest mip, and its `image_size`
//!   is the cooked sum from its first level;
//! * 400 of the 400 sampled assets open through the client's bundle-aware path;
//!   399 decode and 1 names an unsupported format (`0x0C00800CC0D6F2FD`, stored
//!   0x23 → DXGI 34, R16G16_FLOAT). The store-only `bytes_of` path that the old
//!   census used reports 392 decoded / 8 absent once its index is warm (390/2
//!   before it settles) and cannot see bundle-hosted assets; the client's open
//!   finds all 400.
//!
//! No GUID list is an input: the sample is the sorted registry of type `0x004`
//! taken at a fixed stride, the same GUID sample `ow-tank-tex --census 400`
//! selects (the open path here is the client's bundle-aware one).
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test texture_real -- --nocapture
//! ```

use std::collections::BTreeMap;

use ow_tank::keytable::Sources;
use ow_tank::{guid, texture, Error as DataError, GameData, ManifestSelector};

fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

#[test]
fn real_texture_assets_obey_the_client_header_and_payload_invariants() {
    let Some(data) = open() else {
        return;
    };
    let mut guids = data.guids_of_type(guid::types::TEXTURE);
    guids.sort_unstable();
    assert!(!guids.is_empty(), "the registry has type-0x004 assets");
    let step = (guids.len() / 400).max(1);
    let sample: Vec<u64> = guids.iter().copied().step_by(step).take(400).collect();
    assert_eq!(sample.len(), 400, "a 400-asset spread");

    let mut decoded = 0usize;
    let mut unsupported = 0usize;
    let mut payload_missing = 0usize;
    let mut asset_missing = 0usize;
    let mut with_chunk = 0usize;
    let mut embedded_level0 = 0usize;
    let mut payload_level0 = 0usize;
    let mut npot_block = 0usize;
    let mut by_format: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();

    for g in sample {
        let bytes = match data.open_asset(g) {
            Ok(b) => b,
            Err(DataError::NotFound(_)) => {
                asset_missing += 1;
                continue;
            }
            Err(e) => {
                asset_missing += 1;
                failures.push(format!("0x{g:016x}: open: {e}"));
                continue;
            }
        };
        let header = match texture::TextureHeader::parse(&bytes) {
            Ok(h) => h,
            Err(e) => {
                failures.push(format!("0x{g:016x}: header: {e}"));
                continue;
            }
        };

        // The payload chain and the header fields the client keeps. The first
        // check is an install relation, not a proven per-bit meaning of the low
        // mask bits; only bit 3 is read (as the streamed flag).
        if header.payload_count != header.chunk_mask.count_ones() as u8 {
            failures.push(format!(
                "0x{g:016x}: payload_count {} != popcount(chunk_mask 0x{:02x})",
                header.payload_count, header.chunk_mask
            ));
        }
        if header.base_mips > header.mip_count {
            failures.push(format!(
                "0x{g:016x}: base_mips {} > mip_count {}",
                header.base_mips, header.mip_count
            ));
        }
        let (depth, surfaces) = header.depth_and_surfaces();
        let cooked = texture::cooked_size(
            header.format,
            header.width as u32,
            header.height as u32,
            depth,
            header.mip_count as u32,
            surfaces,
        );
        let walk = texture::size_walk(
            header.format,
            header.width as u32,
            header.height as u32,
            depth,
            header.mip_count as u32,
            surfaces,
        );
        if cooked != header.total_size as u64 {
            failures.push(format!(
                "0x{g:016x}: total_size {} but the level sum says {cooked} \
                 ({}x{}, {} mips, {depth} depth, {surfaces} surfaces, format 0x{:02x})",
                header.total_size, header.width, header.height, header.mip_count, header.format
            ));
        }
        // The runtime walk multiplies pixels; it is smaller than the on-disk
        // sum only when a block texture's halved dimensions leave a multiple
        // of 4. It can never be larger.
        if walk != cooked {
            if walk > cooked || !texture::is_block_format(header.format) {
                failures.push(format!(
                    "0x{g:016x}: runtime walk {walk} vs level sum {cooked} \
                     (format 0x{:02x}, {}x{})",
                    header.format, header.width, header.height
                ));
            } else {
                npot_block += 1;
            }
        }

        // The embedded chunk, when there is one.
        if header.payload_count > 0 {
            with_chunk += 1;
            let Some(chunk_bytes) = bytes.get(0x20..) else {
                failures.push(format!("0x{g:016x}: no room for a chunk header"));
                continue;
            };
            let chunk = match texture::TextureChunk::parse(chunk_bytes) {
                Ok(c) => c,
                Err(e) => {
                    failures.push(format!("0x{g:016x}: chunk: {e}"));
                    continue;
                }
            };
            let first = match chunk.first_level(header.mip_count) {
                Some(f) => f,
                None => {
                    failures.push(format!(
                        "0x{g:016x}: chunk first_from_smallest {} past mip_count {}",
                        chunk.first_from_smallest, header.mip_count
                    ));
                    continue;
                }
            };
            if first + chunk.level_count != header.mip_count as u32 {
                failures.push(format!(
                    "0x{g:016x}: chunk levels {first}..{} do not end at mip {}",
                    first + chunk.level_count,
                    header.mip_count
                ));
            }
            let min = if texture::is_block_format(header.format) {
                4
            } else {
                1
            };
            let w = (header.width as u32 >> first).max(min);
            let h = (header.height as u32 >> first).max(min);
            let want =
                texture::cooked_size(header.format, w, h, depth, chunk.level_count, surfaces);
            if want != chunk.image_size as u64 {
                failures.push(format!(
                    "0x{g:016x}: chunk image_size {} but the level sum from level {first} says {want}",
                    chunk.image_size
                ));
            }
            if first == 0 {
                embedded_level0 += 1;
                let end = 0x20usize
                    .saturating_add(chunk.header_size as usize)
                    .saturating_add(chunk.image_size as usize);
                if end > bytes.len() {
                    failures.push(format!(
                        "0x{g:016x}: embedded chunk needs {end} B of {} B",
                        bytes.len()
                    ));
                }
            } else {
                payload_level0 += 1;
            }
        }

        match texture::parse_asset(&bytes, g, &|p| data.open_asset(p).ok()) {
            Ok(t) => {
                decoded += 1;
                *by_format.entry((header.format, t.format_byte)).or_default() += 1;
                if t.format_byte != header.format || t.width != header.width {
                    failures.push(format!("0x{g:016x}: decoded a different header"));
                }
            }
            Err(texture::Error::UnsupportedFormat(id)) => {
                unsupported += 1;
                eprintln!(
                    "  unsupported: 0x{g:016x} stored 0x{:02x} -> DXGI {id} ({}x{}, {} mip)",
                    header.format, header.width, header.height, header.mip_count
                );
                *by_format.entry((header.format, 0xff)).or_default() += 1;
            }
            Err(texture::Error::PayloadNotLoaded(index)) => {
                payload_missing += 1;
                eprintln!(
                    "  payload absent: 0x{g:016x} wants payload {index} \
                     (mips {}, base {}, mask 0x{:02x}, payloads {})",
                    header.mip_count, header.base_mips, header.chunk_mask, header.payload_count
                );
            }
            Err(e) => failures.push(format!("0x{g:016x}: parse_asset: {e}")),
        }
    }

    eprintln!(
        "texture sample: {} in the registry, 400 sampled; {decoded} decoded, \
         {unsupported} unsupported, {payload_missing} payloads absent, \
         {asset_missing} assets absent ({} readable)",
        guids.len(),
        400 - asset_missing
    );
    eprintln!(
        "  {with_chunk} embedded chunk(s): {embedded_level0} hold level 0, \
         {payload_level0} keep it in a payload; {npot_block} non-power-of-two \
         block texture(s) whose runtime walk is below the on-disk sum"
    );
    eprintln!("  stored format -> decoded format: {by_format:?}");
    if !failures.is_empty() {
        eprintln!("  {} invariant failure(s):", failures.len());
        for line in failures.iter().take(20) {
            eprintln!("    {line}");
        }
    }

    assert!(
        failures.is_empty(),
        "{} texture(s) broke a client invariant; first: {:?}",
        failures.len(),
        failures.first()
    );
    assert_eq!(
        decoded + unsupported + payload_missing + asset_missing,
        400,
        "every sampled asset is accounted for"
    );
    // Measured on this installation through `GameData::open_asset`: 205,514
    // texture GUIDs in the registry, 400 sampled at a fixed stride, all 400
    // readable. 399 decode; one (`0x0C00800CC0D6F2FD`, stored 0x23 -> DXGI 34,
    // R16G16_FLOAT) names an unsupported format. No level-0 payload is absent.
    assert_eq!(asset_missing, 0, "the bundle-aware open finds every asset");
    assert_eq!(payload_missing, 0, "no level-0 payload is absent");
    assert_eq!(decoded, 399, "decoded assets");
    assert_eq!(unsupported, 1, "named unsupported formats");
    assert_eq!(
        decoded + unsupported,
        400,
        "every sampled asset is accounted for"
    );
}

/// The six headers the unit test pins are the install's own bytes.
#[test]
fn the_golden_headers_are_the_installs_bytes() {
    let Some(data) = open() else {
        return;
    };
    let golden: [(u64, u64, u32, u8); 6] = [
        // guid, descriptor, total_size, stored format byte
        (0x0c00_0000_0000_09c1, 0x03cb_1580_00ff_c3ff, 699_064, 0x49),
        (0x0c00_0000_0006_a2c6, 0x00c1_8400_501f_c07f, 131_232, 0x60),
        (0x0c00_0000_0003_a505, 0x04ca_2500_007f_c1ff, 349_552, 0x64),
        (0x0c00_0000_0000_64d2, 0x02ba_2400_007f_c1ff, 174_760, 0x51),
        (0x0c00_0000_0004_70e8, 0x05bb_2480_00ff_c3ff, 699_048, 0x51),
        (0x0c00_0000_0004_70d8, 0x04ba_2400_007f_c1ff, 174_760, 0x51),
    ];
    let mut checked = 0usize;
    for (g, descriptor, total_size, stored) in golden {
        let Ok(bytes) = data.open_asset(g) else {
            eprintln!("note: 0x{g:016x} is not present in this installation");
            continue;
        };
        let header = texture::TextureHeader::parse(&bytes).expect("the header parses");
        assert_eq!(header.descriptor(), descriptor, "0x{g:016x}");
        assert_eq!(header.total_size, total_size, "0x{g:016x}");
        assert_eq!(header.format, stored, "0x{g:016x}");
        checked += 1;
    }
    if checked == 0 {
        eprintln!("note: none of the six golden textures is present; nothing to assert");
    }
}

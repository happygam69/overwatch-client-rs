//! The texture mip chain and the array/cubemap layout, against the install.
//!
//! proof-covers: 0x141e296a0 0x142e36e80
//!
//! The client's copy order is read from `0x141e296a0`: the outer loop is the
//! array element (then the cube face), the inner loop is the mip from the
//! largest to the smallest, and the data pointer advances by each level's own
//! byte size. The record index is the D3D12 subresource number
//! `(array * faces + face) * mip_count + mip` (`0x142e36e80`; `faces` is 6 for
//! renderer texture types 4/7), and `0x141f0a000` copies that order with its
//! record-array flag bit 1 set. A separate payload's levels are named by its
//! own chunk header (the same four fields the client reads at
//! `0x142e38730`/`0x142e38740`/`0x142e38750`/`0x142255e10`), not by its index.
//!
//! The pinned assets and the raw six-surface asset have their expected offsets
//! derived from the header in the test (surface stride and per-mip prefix);
//! the synthetic `a_raw_run_is_surface_major_and_mip_ascending` test pins the
//! same order without an install. The 400-asset scan then compares each
//! decoded level to the source at the offset the decoder computed: that proves
//! the level list is complete, in D3D12 subresource order, at the right
//! dimensions/size and contiguous in the source run. It does not independently
//! prove the offset formula for every sampled asset; the four pinned assets
//! and the synthetic test are what pin it.
//!
//! No asset bytes are stored in the repository. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test texture_levels_real -- --nocapture
//! ```

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Instant;

use ow_tank::keytable::Sources;
use ow_tank::{guid, texture, Error as DataError, GameData, ManifestSelector};

const CUBEMAP: u64 = 0x0c00_0000_0006_a2c6;
const SPLIT_BC1: u64 = 0x0c00_0000_0000_09c1;
const STREAMED_BC7: u64 = 0x0c00_0000_0003_a505;
/// A `payload_count == 0` six-surface asset, found by scanning type-`004`
/// registry entries for a raw run with more than one surface. 32x32
/// R16G16B16A16_FLOAT, 6 mips, 6 surfaces, 65,520 B.
const RAW_SIX_SURFACE: u64 = 0x0c00_0000_0001_17c3;

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

fn asset(data: &GameData, g: u64) -> Option<Vec<u8>> {
    match data.open_asset(g) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("note: 0x{g:016x} is not present ({e}); skipping");
            None
        }
    }
}

fn loader<'a>(
    data: &'a GameData,
    cache: &'a RefCell<BTreeMap<u64, Vec<u8>>>,
) -> impl Fn(u64) -> Option<Vec<u8>> + 'a {
    move |p| {
        let b = data.open_asset(p).ok()?;
        cache.borrow_mut().insert(p, b.clone());
        Some(b)
    }
}

/// One level's bytes, from the asset itself or a payload, at the offset the
/// decoder computed. Used by the pinned tests with test-derived offsets and by
/// the 400 scan with the decoder's offsets; in the scan it proves source
/// contiguity, not the offset formula.
fn level_matches(
    level: &texture::TextureLevel,
    parent: &[u8],
    payloads: &BTreeMap<u64, Vec<u8>>,
    guid: u64,
) -> bool {
    let source = match level.source {
        texture::BlockSource::Asset | texture::BlockSource::Embedded => parent,
        texture::BlockSource::Payload(index) => {
            let key = ow_tank::guid::payload_of(guid, index);
            match payloads.get(&key) {
                Some(b) => b,
                None => return false,
            }
        }
    };
    source.get(level.block_offset..level.block_offset + level.blocks.len())
        == Some(level.blocks.as_slice())
}

/// A raw `payload_count == 0` six-surface chain with offsets computed by the
/// test from the header, not from the decoder: surface stride
/// `total_size / surfaces` and the per-mip prefix. This is the independent
/// multi-surface offset check; the synthetic test pins the same order without
/// an install.
#[test]
fn a_raw_six_surface_chain_uses_the_surface_stride_and_mip_order() {
    let Some(data) = open() else {
        return;
    };
    let Some(bytes) = asset(&data, RAW_SIX_SURFACE) else {
        return;
    };
    let header = texture::TextureHeader::parse(&bytes).unwrap();
    assert_eq!(
        (
            header.payload_count,
            header.mip_count,
            header.surfaces,
            header.width,
            header.height,
            header.format
        ),
        (0, 6, 6, 32, 32, 0x0a),
        "the fixture is the raw six-surface asset the scan found"
    );
    assert_eq!(header.total_size, 65_520);

    let t = texture::parse_asset(&bytes, RAW_SIX_SURFACE, &|_| None).expect("raw run decodes");
    assert_eq!(t.levels.len(), 36, "6 surfaces x 6 mips");

    // R16G16B16A16_FLOAT, 8 B/px: 32x32 down to 1x1, explicitly computed.
    let sizes = [8192usize, 2048, 512, 128, 32, 8];
    let dims = [32u16, 16, 8, 4, 2, 1];
    let prefix = [0usize, 8192, 10_240, 10_752, 10_880, 10_912];
    let per_surface: usize = 10_920;
    assert_eq!(sizes.iter().sum::<usize>(), per_surface);
    assert_eq!(per_surface * 6, 65_520);

    for surface in 0..6usize {
        for mip in 0..6usize {
            let level = t
                .level(mip as u8, surface as u16)
                .unwrap_or_else(|| panic!("surface {surface} mip {mip}"));
            let want_at = 0x20 + surface * per_surface + prefix[mip];
            assert_eq!(
                level.block_offset, want_at,
                "surface {surface} mip {mip} offset"
            );
            assert_eq!(
                (level.width, level.height),
                (dims[mip], dims[mip]),
                "surface {surface} mip {mip} dimensions"
            );
            assert_eq!(
                level.blocks.len(),
                sizes[mip],
                "surface {surface} mip {mip} size"
            );
            assert_eq!(level.source, texture::BlockSource::Asset);
            assert_eq!(
                level.blocks,
                bytes[want_at..want_at + sizes[mip]],
                "surface {surface} mip {mip} bytes"
            );
        }
    }
    // The second surface begins one whole 10,920-byte run later, not one mip:
    // a mip-major layout would put it at 0x20 + 8,192.
    assert_eq!(t.level(0, 1).unwrap().block_offset, 0x20 + 10_920);
}

/// 128x128 BC6H, 8 mips, 6 surfaces, one embedded chunk holding every level.
#[test]
fn the_cubemap_decodes_six_faces_by_eight_levels() {
    let Some(data) = open() else {
        return;
    };
    let Some(bytes) = asset(&data, CUBEMAP) else {
        return;
    };
    let cache = RefCell::new(BTreeMap::new());
    let t =
        texture::parse_asset(&bytes, CUBEMAP, &loader(&data, &cache)).expect("the cubemap decodes");
    assert_eq!((t.width, t.height), (128, 128));
    assert_eq!((t.mip_count, t.surfaces), (8, 6));
    assert_eq!(t.levels.len(), 48, "6 faces x 8 mips");
    assert_eq!(t.source, texture::BlockSource::Embedded);
    assert_eq!(t.block_offset, 0x30, "embedded chunk data");
    assert!(level_matches(
        &t.levels[0],
        &bytes,
        &cache.borrow(),
        CUBEMAP
    ));

    // Eight mips, not six: the last three are 4x4, 2x2 and 1x1 resource
    // dimensions, each still one 16-byte block.
    let dims = [128u16, 64, 32, 16, 8, 4, 2, 1];
    let sizes = [16384usize, 4096, 1024, 256, 64, 16, 16, 16];
    let per_face: usize = sizes.iter().sum();
    assert_eq!(per_face, 21872);
    assert_eq!(per_face * 6, 131_232);

    let mut prefix = Vec::with_capacity(8);
    let mut at = 0usize;
    for size in sizes {
        prefix.push(at);
        at += size;
    }
    for face in 0..6usize {
        for (mip, (w, size)) in dims.iter().zip(sizes).enumerate() {
            let level = t
                .level(mip as u8, face as u16)
                .unwrap_or_else(|| panic!("face {face} mip {mip}"));
            let want_at = 0x30 + face * per_face + prefix[mip];
            assert_eq!(level.width, *w, "face {face} mip {mip} width");
            assert_eq!(level.height, *w, "face {face} mip {mip} height");
            assert_eq!(level.block_offset, want_at, "face {face} mip {mip} offset");
            assert_eq!(level.blocks.len(), size, "face {face} mip {mip} size");
            assert_eq!(level.rgba.len(), (*w as usize) * (*w as usize) * 4);
            assert_eq!(level.rgba16.len(), (*w as usize) * (*w as usize) * 4);
            assert_eq!(level.source, texture::BlockSource::Embedded);
            assert_eq!(
                level.blocks,
                bytes[want_at..want_at + size],
                "face {face} mip {mip} bytes"
            );
            // The first block is the file's first block for this subresource.
            assert_eq!(
                &level.blocks[..16.min(size)],
                &bytes[want_at..want_at + 16.min(size)]
            );
        }
        // The list is surface-major, mip-minor: face 1 starts at index 8.
        assert_eq!(t.levels[face * 8].surface, face as u16);
        assert_eq!(t.levels[face * 8].mip, 0);
    }
    assert_eq!(t.rgba, t.levels[0].rgba);
    assert_eq!(t.rgba16, t.levels[0].rgba16);
    assert_eq!(t.blocks, t.levels[0].blocks);
}

/// 1024x1024 BC1, 11 mips, one surface; the embedded chunk holds mips 2..10 and
/// payloads 1 and 2 hold mips 1 and 0.
#[test]
fn the_split_bc1_asset_takes_each_level_from_the_chunk_that_names_it() {
    let Some(data) = open() else {
        return;
    };
    let Some(bytes) = asset(&data, SPLIT_BC1) else {
        return;
    };
    let cache = RefCell::new(BTreeMap::new());
    let t = texture::parse_asset(&bytes, SPLIT_BC1, &loader(&data, &cache))
        .expect("the split asset decodes");
    assert_eq!((t.width, t.height), (1024, 1024));
    assert_eq!((t.mip_count, t.surfaces), (11, 1));
    assert_eq!(t.levels.len(), 11);

    let header = texture::TextureHeader::parse(&bytes).unwrap();
    assert_eq!(header.payload_count, 3);
    assert_eq!(header.requested_payloads(), [1, 2]);
    let embedded = texture::TextureChunk::parse(&bytes[0x20..]).unwrap();
    assert_eq!(embedded.first_level(11), Some(2));
    assert_eq!(embedded.level_count, 9);
    assert_eq!(embedded.image_size, 43_704);

    let payload1 = cache.borrow()[&guid::payload_of(SPLIT_BC1, 1)].clone();
    let payload2 = cache.borrow()[&guid::payload_of(SPLIT_BC1, 2)].clone();
    for (index, bytes) in [(1u32, &payload1), (2, &payload2)] {
        let chunk = texture::TextureChunk::parse(bytes).unwrap();
        assert_eq!(chunk.header_size, 0x10);
        assert_eq!(chunk.level_count, 1);
        assert_eq!(
            chunk.first_level(11),
            Some(if index == 1 { 1 } else { 0 }),
            "payload {index}"
        );
    }
    assert_eq!(
        texture::TextureChunk::parse(&payload2).unwrap().image_size,
        524_288
    );
    assert_eq!(
        texture::TextureChunk::parse(&payload1).unwrap().image_size,
        131_072
    );

    let sizes = [
        524_288usize,
        131_072,
        32_768,
        8_192,
        2_048,
        512,
        128,
        32,
        8,
        8,
        8,
    ];
    let mut embedded_prefix = Vec::new();
    let mut at = 0usize;
    for size in &sizes[2..] {
        embedded_prefix.push(at);
        at += size;
    }
    assert_eq!(at, 43_704);

    for (mip, size) in sizes.iter().enumerate() {
        let level = t.level(mip as u8, 0).expect("every mip");
        assert_eq!(level.blocks.len(), *size, "mip {mip} size");
        match mip {
            0 => {
                assert_eq!(level.source, texture::BlockSource::Payload(2));
                assert_eq!(level.block_offset, 0x10);
                assert_eq!(
                    level.blocks,
                    payload2[0x10..0x10 + size],
                    "mip 0 is payload 2's chunk data"
                );
            }
            1 => {
                assert_eq!(level.source, texture::BlockSource::Payload(1));
                assert_eq!(level.block_offset, 0x10);
                assert_eq!(
                    level.blocks,
                    payload1[0x10..0x10 + size],
                    "mip 1 is payload 1's chunk data"
                );
            }
            _ => {
                assert_eq!(level.source, texture::BlockSource::Embedded);
                let want = 0x30 + embedded_prefix[mip - 2];
                assert_eq!(level.block_offset, want, "mip {mip} embedded offset");
                assert_eq!(level.blocks, bytes[want..want + size]);
            }
        }
    }
    assert_eq!(t.source, texture::BlockSource::Payload(2));
    assert_eq!(t.block_offset, 0x10);
    assert_eq!(t.blocks, payload2[0x10..0x10 + 524_288]);
    // The order in the list is the D3D subresource order (surface, then mip).
    assert!(t.levels.windows(2).all(|w| w[1].mip == w[0].mip + 1));
}

/// 512x512 BC7, 10 mips, one surface; level 0 is the highest payload and is
/// marked streamed (`chunk_mask` bit 3), so the loader's immediate request set
/// is empty. The chunk header still names the level, and the decoder reads it.
#[test]
fn the_streamed_bc7_level_zero_is_placed_by_its_chunk_header() {
    let Some(data) = open() else {
        return;
    };
    let Some(bytes) = asset(&data, STREAMED_BC7) else {
        return;
    };
    let cache = RefCell::new(BTreeMap::new());
    let t = texture::parse_asset(&bytes, STREAMED_BC7, &loader(&data, &cache))
        .expect("the streamed asset decodes");
    assert_eq!((t.width, t.height), (512, 512));
    assert_eq!((t.mip_count, t.surfaces), (10, 1));
    assert_eq!(t.levels.len(), 10);

    let header = texture::TextureHeader::parse(&bytes).unwrap();
    assert_eq!(header.payload_count, 2);
    assert_eq!(header.requested_payloads(), [], "level 0 is streamed");
    assert_eq!(header.chunk_mask & 0x08, 0x08);

    let payload1 = cache.borrow()[&guid::payload_of(STREAMED_BC7, 1)].clone();
    let chunk = texture::TextureChunk::parse(&payload1).unwrap();
    assert_eq!(chunk.first_level(10), Some(0));
    assert_eq!(chunk.level_count, 1);
    assert_eq!(chunk.image_size, 262_144);

    let sizes = [
        262_144usize,
        65_536,
        16_384,
        4_096,
        1_024,
        256,
        64,
        16,
        16,
        16,
    ];
    let dims = [512u16, 256, 128, 64, 32, 16, 8, 4, 2, 1];
    let mut embedded_prefix = Vec::new();
    let mut at = 0usize;
    for size in &sizes[1..] {
        embedded_prefix.push(at);
        at += size;
    }
    assert_eq!(at, 87_408);
    for (mip, (size, w)) in sizes.iter().zip(dims).enumerate() {
        let level = t.level(mip as u8, 0).expect("every mip");
        assert_eq!((level.width, level.height), (w, w), "mip {mip} dimensions");
        assert_eq!(level.blocks.len(), *size, "mip {mip} size");
        if mip == 0 {
            assert_eq!(level.source, texture::BlockSource::Payload(1));
            assert_eq!(
                level.blocks,
                payload1[0x10..0x10 + size],
                "level 0 is the payload's chunk data"
            );
        } else {
            assert_eq!(level.source, texture::BlockSource::Embedded);
            let want = 0x30 + embedded_prefix[mip - 1];
            assert_eq!(level.block_offset, want);
            assert_eq!(level.blocks, bytes[want..want + size]);
        }
    }
}

/// Every sampled asset: the level list is complete, in D3D subresource order,
/// and each level has its own dimensions/size and matches the source run at the
/// offset the decoder computed.
///
/// This proves completeness, order, dimensions, sizes and source contiguity; it
/// does not independently prove the offset formula for each asset. The offsets
/// are pinned by the three pinned assets, the raw six-surface test above and
/// the synthetic `a_raw_run_is_surface_major_and_mip_ascending` test.
#[test]
fn the_400_asset_scan_covers_every_level_and_source_run() {
    let Some(data) = open() else {
        return;
    };
    let started = Instant::now();
    let mut guids = data.guids_of_type(guid::types::TEXTURE);
    guids.sort_unstable();
    let step = (guids.len() / 400).max(1);
    let sample: Vec<u64> = guids.iter().copied().step_by(step).take(400).collect();
    assert_eq!(sample.len(), 400);

    let mut decoded = 0usize;
    let mut unsupported = 0usize;
    let mut missing = 0usize;
    let mut asset_missing = 0usize;
    let mut raw_assets = 0usize;
    let mut levels_total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let cache = RefCell::new(BTreeMap::new());

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
        let Ok(header) = texture::TextureHeader::parse(&bytes) else {
            failures.push(format!("0x{g:016x}: header"));
            continue;
        };
        if header.payload_count == 0 {
            raw_assets += 1;
        }
        cache.borrow_mut().clear();
        match texture::parse_asset(&bytes, g, &loader(&data, &cache)) {
            Ok(t) => {
                decoded += 1;
                let mips = header.mip_count as usize;
                let surfaces = header.surfaces as usize;
                if t.levels.len() != mips * surfaces {
                    failures.push(format!(
                        "0x{g:016x}: {} levels, want {mips} x {surfaces}",
                        t.levels.len()
                    ));
                    continue;
                }
                levels_total += t.levels.len();
                for (index, level) in t.levels.iter().enumerate() {
                    let want_surface = (index / mips) as u16;
                    let want_mip = (index % mips) as u8;
                    if (level.surface, level.mip) != (want_surface, want_mip) {
                        failures.push(format!(
                            "0x{g:016x}: level {index} is (mip {}, surface {})",
                            level.mip, level.surface
                        ));
                        break;
                    }
                    let (w, h) = texture::level_dimensions(
                        header.width as u32,
                        header.height as u32,
                        level.mip as u32,
                    );
                    if (level.width as u32, level.height as u32) != (w, h) {
                        failures.push(format!(
                            "0x{g:016x}: mip {} is {}x{}, want {w}x{h}",
                            level.mip, level.width, level.height
                        ));
                        break;
                    }
                    if level.blocks.len() != texture::level_byte_size(t.format, w, h) {
                        failures.push(format!(
                            "0x{g:016x}: mip {} has {} bytes, want {}",
                            level.mip,
                            level.blocks.len(),
                            texture::level_byte_size(t.format, w, h)
                        ));
                        break;
                    }
                    // True-by-construction at the offset the decoder chose:
                    // this checks the level is drawn from the contiguous source
                    // run, not that the offset formula is independently right
                    // for this asset. The pinned/synthetic tests do that.
                    if !level_matches(level, &bytes, &cache.borrow(), g) {
                        failures.push(format!(
                            "0x{g:016x}: mip {} surface {} bytes differ from {:?}+{:#x}",
                            level.mip, level.surface, level.source, level.block_offset
                        ));
                        break;
                    }
                }
                if (t.level(0, 0).map(|l| &l.blocks) != Some(&t.blocks))
                    || (t.level(0, 0).map(|l| &l.rgba) != Some(&t.rgba))
                {
                    failures.push(format!("0x{g:016x}: mip 0 surface 0 is not the default"));
                }
                if header.payload_count == 0 && t.levels[0].source != texture::BlockSource::Asset {
                    failures.push(format!("0x{g:016x}: raw run is not the asset"));
                }
            }
            Err(texture::Error::UnsupportedFormat(id)) => {
                unsupported += 1;
                eprintln!(
                    "  unsupported: 0x{g:016x} stored 0x{:02x} -> DXGI {id}",
                    header.format
                );
            }
            Err(texture::Error::PayloadNotLoaded(index)) => {
                missing += 1;
                eprintln!(
                    "  payload absent: 0x{g:016x} wants payload {index} (payloads {})",
                    header.payload_count
                );
            }
            Err(e) => failures.push(format!("0x{g:016x}: parse_asset: {e}")),
        }
    }

    let elapsed = started.elapsed();
    eprintln!(
        "texture levels: 400 sampled; {decoded} decoded ({levels_total} subresources), \
         {unsupported} unsupported, {missing} payloads absent, {asset_missing} assets absent, \
         {raw_assets} payload_count==0; {:.1} s",
        elapsed.as_secs_f64()
    );
    if !failures.is_empty() {
        for line in failures.iter().take(20) {
            eprintln!("    {line}");
        }
        panic!("{} texture layout failure(s)", failures.len());
    }
    assert_eq!(asset_missing, 0, "every sampled asset opens");
    assert_eq!(decoded + unsupported + missing, 400);
    // Recorded after this run; the counts are assertions, not inputs.
    assert_eq!(unsupported, 1, "one format is not decoded");
    assert_eq!(missing, 0, "every named payload opens");
    assert_eq!(decoded, 399, "every decodable asset decodes");
    assert_eq!(
        levels_total, 3636,
        "the sampled install's decoded subresource count"
    );
    assert_eq!(raw_assets, 81, "payload_count == 0 assets in the sample");
}

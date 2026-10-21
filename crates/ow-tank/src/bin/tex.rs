//! Decode textures from the install to PNG, and census their formats.
//!
//! ```text
//! ow-tank-tex --guid 0x0c00000000038c31 --png D:/ow-dumps/tex.png
//! ow-tank-tex --census 800
//! ```
//!
//! Read-only: opens the install the same way the rest of the tree does and
//! writes only what `--png` names.

use std::io::Write;

use ow_tank::{guid, texture, GameData, ManifestSelector};

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        let mut c = (crc ^ *byte as u32) & 0xFF;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        crc = c ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn chunk(tag: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    // The PNG CRC covers the chunk *type* and its body.
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(tag);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

fn u32a_of(d: &[u8], at: usize) -> usize {
    d.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        .unwrap_or(0)
}

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0); // filter: none
        let at = y * w as usize * 4;
        raw.extend_from_slice(&rgba[at..at + w as usize * 4]);
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&raw)?;
    let compressed = z.finish()?;

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IDAT", &compressed));
    png.extend(chunk(b"IEND", &[]));
    std::fs::write(path, png)
}

/// The container `dev/bc6h_oracle.py` reads: a BC6H block run next to *our*
/// half-float decode of it. The same shape as the OWTX container above, with
/// the 16-bit plane instead of the 8-bit one (a BC6H pixel has no byte form,
/// so clamping to one before comparing would throw away exactly the range this
/// format exists for).
///
/// ```text
/// +0x00 "OWT6"  +0x04 u32 w  +0x08 u32 h  +0x0c u8 stored format byte
/// +0x0d u8 signed  +0x0e u16 blocks per mode (0 for a texture dump)
/// +0x10 u32 block bytes  +0x14 u32 reserved  +0x18 blocks
/// then w*h*4 u16 little-endian half floats
/// ```
fn write_owt6(
    path: &str,
    w: usize,
    h: usize,
    format_byte: u8,
    signed: bool,
    per_mode: u16,
    blocks: &[u8],
    half: &[u16],
) {
    let mut out = Vec::with_capacity(24 + blocks.len() + half.len() * 2);
    out.extend_from_slice(b"OWT6");
    out.extend_from_slice(&(w as u32).to_le_bytes());
    out.extend_from_slice(&(h as u32).to_le_bytes());
    out.push(format_byte);
    out.push(signed as u8);
    out.extend_from_slice(&per_mode.to_le_bytes());
    out.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(blocks);
    for h16 in half {
        out.extend_from_slice(&h16.to_le_bytes());
    }
    match std::fs::write(path, &out) {
        Ok(()) => println!(
            "wrote {path}: {w}x{h}, {} block bytes, {} half-float channels",
            blocks.len(),
            half.len()
        ),
        Err(e) => eprintln!("write {path} failed: {e}"),
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.is_empty() {
        eprintln!("usage: ow-tank-tex (--guid 0x… [--png FILE]) | --census N");
        return std::process::ExitCode::from(2);
    }

    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = match GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    // Identify the format ids we cannot decode yet by arithmetic: a texture's
    // level-0 byte count over its pixel count is its bytes per pixel, and that
    // number names the format far more reliably than the id does.
    if let Some(n) = flag("--unknown").and_then(|v| v.parse::<usize>().ok()) {
        let mut guids = data.guids_of_type(guid::types::TEXTURE);
        guids.sort_unstable();
        let step = (guids.len() / n.max(1)).max(1);
        let mut by_format: std::collections::BTreeMap<u8, Vec<(u32, u32, usize, usize)>> =
            Default::default();
        let mut tried = 0usize;
        for g in guids.iter().copied().step_by(step) {
            if tried >= n {
                break;
            }
            tried += 1;
            let Ok(bytes) = data.bytes_of(g) else {
                continue;
            };
            // The header alone: a decode is exactly what fails for the formats
            // this is here to identify.
            if bytes.len() < 0x20 {
                continue;
            }
            let (format, _) = texture::Format::from_header(bytes[3]);
            if !matches!(format, texture::Format::Unknown(_)) {
                continue;
            }
            let mut level0 = bytes.len() - 0x20;
            let Some((w, h, _)) = texture::header_dimensions(&bytes) else {
                continue;
            };
            // A separate payload carries the level-0 blocks; its own length is
            // the number this wants.
            for idx in 1..4u32 {
                let payload = guid::payload_of(g, idx);
                if let Ok(b) = data.bytes_of(payload) {
                    if b.len() > 0x20 {
                        level0 = b.len() - 0x20;
                        break;
                    }
                }
            }
            let pixels = (w as usize) * (h as usize);
            let surf = bytes[4].max(1) as usize;
            by_format.entry(bytes[3]).or_default().push((
                w as u32,
                h as u32,
                level0,
                pixels * surf,
            ));
        }
        println!("{tried} texture(s) sampled; unsupported formats:");
        for (byte, samples) in &by_format {
            let (format, id) = texture::Format::from_header(*byte);
            let _ = format;
            let sizes: Vec<String> = samples
                .iter()
                .take(6)
                .map(|(w, h, len, px)| {
                    format!(
                        "{w}x{h} {len} B = {:.2} B/px",
                        *len as f64 / (*px as f64).max(1.0)
                    )
                })
                .collect();
            println!(
                "  stored {byte} (DXGI {id}): {} texture(s) -- {}",
                samples.len(),
                sizes.join(", ")
            );
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(n) = flag("--census").and_then(|v| v.parse::<usize>().ok()) {
        let mut guids = data.guids_of_type(guid::types::TEXTURE);
        guids.sort_unstable();
        // Spread the sample across the whole type. Readability and format
        // differ by which package an asset sits in, so a prefix is not a sample
        // of the type -- it is a sample of one corner of it.
        let step = (guids.len() / n.max(1)).max(1);
        let picked: Vec<u64> = guids.iter().copied().step_by(step).take(n).collect();
        let mut families: std::collections::BTreeMap<String, usize> = Default::default();
        let mut decoded = 0usize;
        let mut failed = 0usize;
        let mut shown = 0usize;
        for g in picked {
            match data.bytes_of(g).map_err(|e| e.to_string()).and_then(|b| {
                texture::parse_asset(&b, g, &|p| data.bytes_of(p).ok()).map_err(|e| e.to_string())
            }) {
                Ok(t) => {
                    decoded += 1;
                    if decoded <= 4 {
                        println!(
                            "    decodes: 0x{g:016x}: {}x{} {:?} (byte {})",
                            t.width, t.height, t.format, t.format_byte
                        );
                    }
                    *families
                        .entry(format!("{:?} (byte {})", t.format, t.format_byte))
                        .or_insert(0) += 1;
                }
                Err(e) => {
                    failed += 1;
                    *families.entry(format!("FAILED: {e}")).or_insert(0) += 1;
                    if shown < 4 && format!("{e}").contains("block data") {
                        shown += 1;
                        if let Ok(b) = data.bytes_of(g) {
                            let u16a = |at: usize| {
                                b.get(at..at + 2)
                                    .map(|s| u16::from_le_bytes([s[0], s[1]]))
                                    .unwrap_or(0)
                            };
                            println!(
                                "    e.g. 0x{g:016x}: asset {} B, mips {}, fmt {}, surfaces {}, payloads {}, data_size {}, {}x{}",
                                b.len(), b[2], b[3], b[4], b[6], u16a(0x0c), u16a(8), u16a(10)
                            );
                        }
                    }
                }
            }
        }
        println!("census over {n} texture assets: {decoded} decoded, {failed} failed");
        let mut rows: Vec<(String, usize)> = families.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, c) in rows {
            println!("  {c:5}  {k}");
        }
        return std::process::ExitCode::SUCCESS;
    }

    // BC6H self-test sweep: build synthetic blocks for all fourteen modes,
    // decode them, and write the exact bytes next to our own half-float output
    // so `dev/bc6h_oracle.py` can hand the same blocks to an independent
    // decoder. BC6H decoding is total (every 128-bit value decodes to
    // something), so a block with the right mode number and a random payload
    // is a valid input — and a random payload is what reaches delta wraps and
    // the unquantisation clamps.
    if let Some(path) = flag("--bc6h-selftest") {
        let per_mode = flag("--per-mode")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(64);
        let signed = args.iter().any(|a| a == "--signed");
        let mut blocks: Vec<u8> = Vec::new();
        let mut half: Vec<u16> = Vec::new();
        // A 4 x (4 * count) image of stacked blocks, one group per mode.
        let height = 4 * per_mode * texture::BC6H_MODE_NUMBERS.len() as usize;
        for mode in texture::BC6H_MODE_NUMBERS {
            for block in texture::bc6h_random_blocks(mode, per_mode, 0x5EED) {
                blocks.extend_from_slice(&block);
                let (_, halves) = texture::decode_bc6h_block(&block, signed);
                half.extend_from_slice(&halves);
            }
        }
        write_owt6(
            &path,
            4,
            height,
            if signed { 96 } else { 95 },
            signed,
            per_mode as u16,
            &blocks,
            &half,
        );
        println!(
            "wrote {path}: {} blocks ({} per mode over {:?}), {} half-float channels, signed={signed}",
            blocks.len() / 16,
            per_mode,
            texture::BC6H_MODE_NUMBERS,
            half.len()
        );
        return std::process::ExitCode::SUCCESS;
    }

    // Dump a spread sample for the cross-decoder check (`dev/tex_oracle.py`):
    // the exact level-0 bytes and our decode of them, per texture file.
    if let Some(n) = flag("--dump-spread").and_then(|v| v.parse::<usize>().ok()) {
        let dir = flag("--dir").unwrap_or_else(|| ".".to_string());
        let mut guids = data.guids_of_type(guid::types::TEXTURE);
        guids.sort_unstable();
        let step = (guids.len() / n.max(1)).max(1);
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        let mut written = 0usize;
        for g in guids.iter().copied().step_by(step) {
            if written >= n {
                break;
            }
            let Ok(bytes) = data.bytes_of(g) else {
                continue;
            };
            let Ok(t) = texture::parse_asset(&bytes, g, &|p| data.bytes_of(p).ok()) else {
                continue;
            };
            let mut out = Vec::new();
            out.extend_from_slice(b"OWTX");
            out.extend_from_slice(&(t.width as u32).to_le_bytes());
            out.extend_from_slice(&(t.height as u32).to_le_bytes());
            out.push(t.format_byte);
            out.extend_from_slice(&(t.blocks.len() as u32).to_le_bytes());
            out.extend_from_slice(&t.blocks);
            out.extend_from_slice(&t.rgba);
            let path = format!("{dir}/spread_{g:016x}.dump");
            match std::fs::write(&path, &out) {
                Ok(()) => {
                    written += 1;
                    *counts
                        .entry(format!("{:?} (byte {})", t.format, t.format_byte))
                        .or_default() += 1;
                    println!(
                        "  {path}: {}x{} {:?} from {:?}+{:#x}",
                        t.width, t.height, t.format, t.source, t.block_offset
                    );
                }
                Err(e) => eprintln!("  write {path} failed: {e}"),
            }
        }
        println!("wrote {written} dump(s) to {dir}");
        for (k, c) in counts {
            println!("  {c:4}  {k}");
        }
        return std::process::ExitCode::SUCCESS;
    }

    // The same dump for a spread of the BC6H assets themselves, so the
    // cross-decoder check runs over real encoder output rather than only over
    // synthetic blocks.
    if let Some(n) = flag("--dump16-spread").and_then(|v| v.parse::<usize>().ok()) {
        let dir = flag("--dir").unwrap_or_else(|| ".".to_string());
        let mut guids = data.guids_of_type(guid::types::TEXTURE);
        guids.sort_unstable();
        let step = (guids.len() / n.max(1)).max(1);
        let mut written = 0usize;
        let mut skipped = 0usize;
        for g in guids.iter().copied().step_by(step) {
            if written >= n {
                break;
            }
            let Ok(bytes) = data.bytes_of(g) else {
                skipped += 1;
                continue;
            };
            let Ok(t) = texture::parse_asset(&bytes, g, &|p| data.bytes_of(p).ok()) else {
                skipped += 1;
                continue;
            };
            let signed = match t.format {
                texture::Format::Bc6h { signed } => signed,
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            write_owt6(
                &format!("{dir}/bc6h_{g:016x}.owt6"),
                t.width as usize,
                t.height as usize,
                t.format_byte,
                signed,
                0,
                &t.blocks,
                &t.rgba16,
            );
            println!(
                "  {dir}/bc6h_{g:016x}.owt6: {}x{} signed={signed} from {:?}+{:#x}",
                t.width, t.height, t.source, t.block_offset
            );
            written += 1;
        }
        println!("wrote {written} BC6H dump(s) to {dir} ({skipped} non-BC6H or unreadable)");
        return std::process::ExitCode::SUCCESS;
    }

    // Debug: how a texture stores its chain — the asset header, its payload
    // records, and where a BC-block-shaped run actually starts. The embedded
    // run is *not* always the data (a 256x256 BC7 hero texture keeps a 16-byte
    // descriptor table there), so this prints candidates instead of arguing.
    if args.iter().any(|a| a == "--header") {
        let Some(g) =
            flag("--guid").and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        else {
            eprintln!("--header wants --guid 0x…");
            return std::process::ExitCode::from(2);
        };
        let bytes = match data.bytes_of(g) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("load failed: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let u16a = |at: usize| {
            bytes
                .get(at..at + 2)
                .map(|s| u16::from_le_bytes([s[0], s[1]]))
                .unwrap_or(0)
        };
        let u32a = |at: usize| {
            bytes
                .get(at..at + 4)
                .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
                .unwrap_or(0)
        };
        println!(
            "texture 0x{g:016x}: {} B, mips {}, format {}, surfaces {}, usage {}, payloads {}, data_size {}, {}x{}",
            bytes.len(),
            bytes.get(2).copied().unwrap_or(0),
            bytes.get(3).copied().unwrap_or(0),
            bytes.get(4).copied().unwrap_or(0),
            bytes.get(5).copied().unwrap_or(0),
            bytes.get(6).copied().unwrap_or(0),
            u16a(0x0c),
            u16a(8),
            u16a(10)
        );
        println!(
            "  at +0x20: {} {} {} {}",
            u32a(0x20),
            u32a(0x24),
            u32a(0x28),
            u32a(0x2c)
        );
        let head: Vec<String> = bytes
            .iter()
            .take(0x40)
            .map(|b| format!("{b:02x}"))
            .collect();
        println!("  first 0x40: {}", head.join(" "));
        if let Ok((start, size)) = texture::block_span(&bytes) {
            println!("  block_span: start {start:#x}, size {size}");
        }
        // Which offsets hold something that looks like BC7 blocks: a valid
        // block's first byte has a set bit in its low three bits (mode 0-7).
        let mut offsets: Vec<(usize, usize)> = Vec::new();
        for start in (0x20..0x400usize).step_by(4) {
            let n = (bytes.len().saturating_sub(start)) / 16;
            let n = n.min(256);
            if n < 16 {
                break;
            }
            let valid = (0..n)
                .filter(|i| {
                    let b = bytes[start + i * 16];
                    b & 0x07 != 0 && b & 0x07 != 0x08
                })
                .count();
            if valid * 10 >= n * 9 {
                offsets.push((start, valid));
            }
        }
        println!("  runs that are >=90% valid-looking BC blocks: {offsets:?}");
        for idx in 1..4u32 {
            let p = guid::payload_of(g, idx);
            match data.bytes_of(p) {
                Ok(b) => {
                    let start = u32a_of(&b, 12);
                    let image = u32a_of(&b, 8);
                    println!(
                        "  payload {idx} 0x{p:016x}: {} B, at +0x0c {start}, at +0x08 {image}{}",
                        b.len(),
                        if start + image <= b.len() {
                            ""
                        } else {
                            "  (overruns)"
                        }
                    );
                }
                Err(_) => println!("  payload {idx} 0x{p:016x}: not present"),
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    // Debug: which payload-GUID encoding resolves for a texture?
    if let Some(t) = flag("--payload-search")
        .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
    {
        println!(
            "texture 0x{t:016x}: type 0x{:03x} index {} locale bits {:#x}",
            guid::type_of(t),
            guid::index_of(t),
            (t >> 32) & 0x1f
        );
        for ty in [0x04Du16, 0x04C, 0x04E] {
            for idx in 0u64..4 {
                for bit40 in [0u64, 1] {
                    let mut g = t & !(0xF_u64 << 32) & !(1_u64 << 40);
                    g |= idx << 32;
                    g |= bit40 << 40;
                    g &= !(0x0FFF_u64 << 48);
                    g |= (guid::reverse_type_bits(ty - 1) as u64) << 48;
                    let ok = data.bytes_of(g).map(|b| b.len()).unwrap_or(0);
                    if ok > 0 {
                        println!("  resolves: type 0x{ty:03x} idx {idx} bit40 {bit40} -> 0x{g:016x} ({} B)", ok);
                    }
                }
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let Some(g) =
        flag("--guid").and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
    else {
        eprintln!("--guid 0x… or --census N");
        return std::process::ExitCode::from(2);
    };
    let bytes = match data.bytes_of(g) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("load failed: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    match texture::parse_asset(&bytes, g, &|p| data.bytes_of(p).ok()) {
        Ok(t) => {
            println!(
                "texture 0x{g:016x}: {}x{}, {} mip(s), {} surface(s), {:?} (byte {}), {} bytes decoded",
                t.width,
                t.height,
                t.mip_count,
                t.surfaces,
                t.format,
                t.format_byte,
                t.rgba.len()
            );
            // A dump for the cross-decoder check (`dev/tex_oracle.py`): the
            // exact level-0 bytes the decode used, next to our own decode, so
            // an independent decoder can be pointed at the same input.
            if let Some(path) = flag("--dump") {
                let mut out = Vec::new();
                out.extend_from_slice(b"OWTX");
                out.extend_from_slice(&(t.width as u32).to_le_bytes());
                out.extend_from_slice(&(t.height as u32).to_le_bytes());
                out.push(t.format_byte);
                out.extend_from_slice(&(t.blocks.len() as u32).to_le_bytes());
                out.extend_from_slice(&t.blocks);
                out.extend_from_slice(&t.rgba);
                match std::fs::write(&path, &out) {
                    Ok(()) => println!(
                        "wrote {path}: {} block bytes from {:?}+{:#x} + {} decoded bytes",
                        t.blocks.len(),
                        t.source,
                        t.block_offset,
                        t.rgba.len()
                    ),
                    Err(e) => eprintln!("write failed: {e}"),
                }
            }
            // The 16-bit dump for a real BC6H asset, the same container the
            // synthetic sweep writes: the exact level-0 blocks next to our own
            // half floats.
            if let Some(path) = flag("--dump16") {
                if t.rgba16.is_empty() {
                    eprintln!(
                        "--dump16 wants a half-float format; {} has none",
                        t.format_byte
                    );
                } else {
                    write_owt6(
                        &path,
                        t.width as usize,
                        t.height as usize,
                        t.format_byte,
                        matches!(t.format, texture::Format::Bc6h { signed: true }),
                        0,
                        &t.blocks,
                        &t.rgba16,
                    );
                }
            }
            if let Some(path) = flag("--png") {
                // A single-channel format decodes into red (the D3D channel
                // position). For an eyeball PNG, promote it to greyscale — the
                // decode stays what the shader would see.
                let shown = if t.format.single_channel() {
                    let mut g = t.rgba.clone();
                    for px in g.chunks_exact_mut(4) {
                        px[1] = px[0];
                        px[2] = px[0];
                    }
                    g
                } else {
                    t.rgba.clone()
                };
                match write_png(&path, t.width as u32, t.height as u32, &shown) {
                    Ok(()) => println!("wrote {path}"),
                    Err(e) => eprintln!("write failed: {e}"),
                }
            }
        }
        Err(e) => {
            println!("texture 0x{g:016x}: {e}");
            return std::process::ExitCode::from(1);
        }
    }
    std::process::ExitCode::SUCCESS
}

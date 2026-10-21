//! A minimal PNG writer.
//!
//! Only what is needed to look at a rendered frame from a script: 8-bit RGBA,
//! no interlacing, one filter type. Writing it here keeps the dependency list
//! at what the project actually needs.

use std::io::Write;

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc = table[((crc ^ *byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// Write RGBA8 pixels, top row first, as a PNG.
pub fn write_rgba(path: &str, width: u32, height: u32, pixels: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((width * height * 4 + height) as usize);
    for row in 0..height {
        raw.push(0); // filter: none
        let start = (row * width * 4) as usize;
        raw.extend_from_slice(&pixels[start..start + (width * 4) as usize]);
    }

    let mut zlib = Vec::new();
    {
        let mut encoder = flate2::write::ZlibEncoder::new(&mut zlib, flate2::Compression::fast());
        encoder.write_all(&raw)?;
        encoder.finish()?;
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);

    std::fs::write(path, out)
}

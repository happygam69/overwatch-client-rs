//! BLTE decoding, including the Salsa20-encrypted chunk mode.
//!
//! A stored payload is either raw `BLTE` or preceded by a 30-byte data header
//! (`md5[16]`, `size u32 LE`, 10 further bytes). After the magic comes a
//! big-endian frame-header size. When it is non-zero the client's
//! `BlteFrameHeaderParse` (`Overwatch.exe` `0x1424a90e0`) requires:
//!
//! * at least `0x24` bytes;
//! * the `BLTE` magic;
//! * a non-zero header size equal to `12 + record_size * count`;
//! * frame type `0x0F` (24-byte records) or `0x10` (40-byte records);
//! * a non-zero `u24` count at `+9`;
//! * the declared header bytes present.
//!
//! The parser hashes exactly `header_size` bytes with MD5 and compares the
//! digest only when the caller supplies an expected hash. Each record is
//! `stored u32 BE`, `decoded u32 BE`, `md5[16]`; type `0x10` adds a further
//! `extra[16]`. Chunk 0 starts at `header_size`; no hash sits between the
//! table and the first chunk. The read path never compares the per-chunk
//! MD5s, and no read-path consumer of the type-`0x10` extras was found.
//!
//! Each chunk starts with a one-byte mode:
//!
//! * `N` stored
//! * `Z` zlib
//! * `E` encrypted: `u8 key-name-len`, key name, `u8 iv-len`, iv, `u8 'S'`,
//!   then Salsa20 ciphertext whose plaintext begins with the real mode byte.
//! * `4` LZ4: `'4'`, version `0x01`, `u64 BE` size (checked <= `0xffffffff`),
//!   `u8` block-size exponent in `5..=16`, then LZ4 block data.
//!
//! There is no `A` case in the client's block reader. `F`/`O` are rejected by
//! the top-level direct reader, not treated as block modes.
//!
//! The chunk ordinal is folded into the first four IV bytes so identical chunks
//! do not encrypt identically.

use std::collections::BTreeMap;
use std::io::Read;

pub const BLTE_MAGIC: [u8; 4] = *b"BLTE";
/// Size of the optional header in front of the BLTE payload.
pub const DATA_HEADER_SIZE: usize = 30;

/// Frame type with 24-byte records: `stored`, `decoded`, `md5[16]`.
pub const FRAME_TYPE_NORMAL: u8 = 0x0F;
/// Frame type with 40-byte records: the normal record plus `extra[16]`.
pub const FRAME_TYPE_EXTENDED: u8 = 0x10;
/// The client rejects a frame-header buffer shorter than this before parsing.
pub const FRAME_HEADER_MIN_SIZE: usize = 0x24;

#[derive(Debug)]
pub enum Error {
    NotBlte,
    Malformed(&'static str),
    MissingKey(u64),
    Zlib(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotBlte => write!(f, "payload is not a BLTE container"),
            Error::Malformed(m) => write!(f, "malformed BLTE: {m}"),
            Error::MissingKey(k) => write!(f, "no keyring key 0x{k:016x} for an encrypted chunk"),
            Error::Zlib(m) => write!(f, "zlib error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// One chunk-table record, as the client parses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameChunk {
    /// Stored byte count, including the mode byte and any encryption header.
    pub stored: u32,
    /// Decoded byte count after the mode byte has been consumed.
    pub decoded: u32,
    /// Per-chunk MD5. The read path parses and copies it but never compares it.
    pub md5: [u8; 16],
    /// Type-`0x10` extra bytes, stored in the client's parallel array.
    pub extra: Option<[u8; 16]>,
}

/// A parsed BLTE frame header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    /// Declared frame-header size, which is also the offset of chunk 0.
    pub header_size: usize,
    /// `0x0F` or `0x10`.
    pub frame_type: u8,
    /// MD5 over exactly `header_size` bytes.
    pub hash: [u8; 16],
    /// The records in file order.
    pub chunks: Vec<FrameChunk>,
}

/// Why `parse_frame_header` did not return a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameHeaderError {
    /// The client's status 7: the buffer is shorter than `needed`.
    NeedMore { needed: usize },
    /// The client's status 8: a field is invalid.
    Invalid(&'static str),
}

impl std::fmt::Display for FrameHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameHeaderError::NeedMore { needed } => write!(f, "need {needed} bytes"),
            FrameHeaderError::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for FrameHeaderError {}

/// Parse a BLTE frame header exactly as `BlteFrameHeaderParse` (`0x1424a90e0`).
///
/// `expected_hash` is the caller's optional digest. The client passes a
/// pointer and a byte length; every read-path caller uses 16. A shorter slice
/// compares that many bytes, and a longer one is rejected because the client's
/// `memcmp` would read past the 16-byte digest.
///
/// The return value mirrors the client's two failure statuses: `NeedMore` is
/// status 7 with `outNeeded = 0x24` or `headerSize`; `Invalid` is status 8.
pub fn parse_frame_header(
    data: &[u8],
    expected_hash: Option<&[u8]>,
) -> std::result::Result<FrameHeader, FrameHeaderError> {
    if data.len() < FRAME_HEADER_MIN_SIZE {
        return Err(FrameHeaderError::NeedMore {
            needed: FRAME_HEADER_MIN_SIZE,
        });
    }
    if data[0..4] != BLTE_MAGIC {
        return Err(FrameHeaderError::Invalid("bad BLTE magic"));
    }
    let header_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if header_size == 0 {
        return Err(FrameHeaderError::Invalid("zero frame-header size"));
    }
    let frame_type = data[8];
    let record_size = match frame_type {
        FRAME_TYPE_NORMAL => 24,
        FRAME_TYPE_EXTENDED => 40,
        _ => return Err(FrameHeaderError::Invalid("unsupported frame type")),
    };
    let count = (usize::from(data[9]) << 16) | (usize::from(data[10]) << 8) | usize::from(data[11]);
    if count == 0 {
        return Err(FrameHeaderError::Invalid("zero chunk count"));
    }
    let expected_header_size = 12 + record_size * count;
    if header_size != expected_header_size {
        return Err(FrameHeaderError::Invalid("frame-header size mismatch"));
    }
    if data.len() < header_size {
        return Err(FrameHeaderError::NeedMore {
            needed: header_size,
        });
    }

    // The client hashes the whole header before it looks at the expected
    // digest, even when the check flag is zero.
    let hash = md5(&data[..header_size]);
    if let Some(expected) = expected_hash {
        if expected.len() > hash.len() {
            return Err(FrameHeaderError::Invalid(
                "expected hash longer than 16 bytes",
            ));
        }
        if hash[..expected.len()] != expected[..] {
            return Err(FrameHeaderError::Invalid("frame-header hash mismatch"));
        }
    }

    let mut chunks = Vec::with_capacity(count);
    for index in 0..count {
        let at = 12 + index * record_size;
        let stored = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let decoded = u32::from_be_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]);
        let mut chunk_md5 = [0u8; 16];
        chunk_md5.copy_from_slice(&data[at + 8..at + 24]);
        let extra = if frame_type == FRAME_TYPE_EXTENDED {
            let mut extra = [0u8; 16];
            extra.copy_from_slice(&data[at + 24..at + 40]);
            Some(extra)
        } else {
            None
        };
        chunks.push(FrameChunk {
            stored,
            decoded,
            md5: chunk_md5,
            extra,
        });
    }

    Ok(FrameHeader {
        header_size,
        frame_type,
        hash,
        chunks,
    })
}

/// Strip a data header if present and return the BLTE container.
pub fn strip_data_header(data: &[u8]) -> Result<&[u8]> {
    if data.len() >= 4 && data[0..4] == BLTE_MAGIC {
        return Ok(data);
    }
    if data.len() < DATA_HEADER_SIZE + 4 {
        return Err(Error::NotBlte);
    }
    let body = &data[DATA_HEADER_SIZE..];
    if body[0..4] != BLTE_MAGIC {
        return Err(Error::NotBlte);
    }
    Ok(body)
}

/// One entry of the chunk table: stored length, and decoded length when known.
struct Chunk {
    stored: usize,
    decoded: Option<usize>,
}

/// Decode a stored payload (data header and all) into plaintext bytes.
pub fn decode(data: &[u8], keyring: &BTreeMap<u64, [u8; 16]>) -> Result<Vec<u8>> {
    // A record whose payload is empty is stored as the 30-byte data header
    // and nothing else. The client reads that as a zero-length file: in
    // `DynamicContainerRead` the header is skipped (`+ 0x1e`) and a
    // zero-length request succeeds without ever opening a BLTE container.
    // Measured: 20 such records in a 20,000-entry sample of this install.
    // `Store::read_payload` already skips the header, so an empty payload is
    // the same record seen through the client's own range.
    if data.is_empty() || (data.len() == DATA_HEADER_SIZE && !data.starts_with(&BLTE_MAGIC)) {
        return Ok(Vec::new());
    }
    let blte = strip_data_header(data)?;
    if blte.len() < 8 {
        return Err(Error::Malformed("truncated container"));
    }
    let frame_header_size = be_u32(blte, 4)? as usize;

    let (chunks, mut pos) = if frame_header_size == 0 {
        // `BlteIsDirectContainer` (`0x1424d8720`): magic plus a zero frame
        // header is the single implicit-chunk form. Its decoded size is not
        // declared, so accept whatever the mode block yields.
        (
            vec![Chunk {
                stored: blte.len() - 8,
                decoded: None,
            }],
            8usize,
        )
    } else {
        // No expected hash is invented here: the read path parses the header
        // with the client's check flag clear. Callers that have a digest use
        // `parse_frame_header` directly.
        let frame = parse_frame_header(blte, None).map_err(frame_header_error)?;
        let chunks = frame
            .chunks
            .iter()
            .map(|chunk| Chunk {
                stored: chunk.stored as usize,
                decoded: Some(chunk.decoded as usize),
            })
            .collect();
        (chunks, frame.header_size)
    };

    let mut out = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        if pos + chunk.stored > blte.len() {
            return Err(Error::Malformed("chunk runs past the container"));
        }
        let raw = &blte[pos..pos + chunk.stored];
        pos += chunk.stored;
        if raw.is_empty() {
            return Err(Error::Malformed("empty chunk"));
        }

        // Decrypting yields an owned buffer whose first byte is the real mode,
        // so keep the working slice in a variable that outlives the borrow.
        let decrypted;
        let block_slice = if raw[0] == b'E' {
            decrypted = decrypt_chunk(raw, index, keyring)?;
            if decrypted.is_empty() {
                return Err(Error::Malformed("empty decrypted chunk"));
            }
            decrypted.as_slice()
        } else {
            raw
        };
        let mode = block_slice[0];
        let body = &block_slice[1..];

        let block = match mode {
            b'N' => body.to_vec(),
            b'Z' => {
                // **`flate2`, not a 1:1 port.** The client's own inflate
                // (`0x1425233d0`, zlib 1.3) is read and a port is in progress in
                // `crate::inflate`, but it does not yet decode every conforming
                // stream (it fails on some repetitive payloads and on install
                // data), so the shipped `Z` path stays on the reference-derived
                // decoder. See `docs/formats/casc.md`.
                let mut buf = Vec::new();
                let mut decoder = flate2::read::ZlibDecoder::new(body);
                // Cap the expansion at the declared decoded size (+1 so an
                // over-long body is caught by the size check below rather
                // than read to the end): a corrupt chunk must not turn a
                // small payload into a decompression bomb.
                match chunk.decoded {
                    Some(expected) => {
                        decoder
                            .take(expected as u64 + 1)
                            .read_to_end(&mut buf)
                            .map_err(|e| Error::Zlib(e.to_string()))?;
                    }
                    None => {
                        decoder
                            .read_to_end(&mut buf)
                            .map_err(|e| Error::Zlib(e.to_string()))?;
                    }
                }
                buf
            }
            b'4' => decode_lz4_chunk(block_slice, chunk.decoded)?,
            _ => return Err(Error::Malformed("unknown BLTE chunk mode")),
        };

        if let Some(expected) = chunk.decoded {
            if block.len() != expected {
                return Err(Error::Malformed("decoded chunk size mismatch"));
            }
        }
        out.extend_from_slice(&block);
    }
    Ok(out)
}

fn frame_header_error(error: FrameHeaderError) -> Error {
    match error {
        FrameHeaderError::NeedMore { .. } => Error::Malformed("truncated frame header"),
        FrameHeaderError::Invalid(message) => Error::Malformed(message),
    }
}

/// `BlteBlockReadStored` (exe `0x1424d7850`): the stored (`N`) block reader,
/// one call at a time, with the client's own state object.
///
/// The read, instruction for instruction:
///
/// - `*out_decoded = 0; *out_consumed = 0;`
/// - a null `dst` or a null `src` returns 3 (the probe cannot express a null
///   pointer: its empty buffer is a non-null arena address, so the Rust drops
///   this check and the null case is an acknowledged gap);
/// - `end = src + src_len`;
/// - the state byte at `+0x10` is the mode-consumed flag: when it is non-zero
///   the reader consumes the mode byte (a `src_len` below 1 returns 0, a mode
///   other than `N` returns 8, and `N` stores 0 at `+0x10` and adds 1 to
///   `*out_consumed`); when it is zero the mode byte is already gone;
/// - a zero `dst_len`, or `src == end`, returns 0;
/// - the copy length is `min(end - src, dst_len)`;
/// - `memcpy(dst, src, len)` (the client's `0x142efe2b0`);
/// - `*out_decoded += len; *out_consumed += len;` and the dword at state
///   `+0xc` (the running decoded total) `+= len`;
/// - returns 0.
///
/// The state object is the client's 0x18-byte block-reader object: `+0x08` is
/// the declared decoded size (written by the initializer `0x1424d7840`), `+0x0c`
/// is the running total this function advances, and `+0x10` is the flag. The
/// reader itself reads only `+0x10` and writes `+0x0c`/`+0x10`.
///
/// `BlteBlockRead` (`0x1424d61b0`) reaches this through the object's vtable
/// slot `[rax + 8]` at `0x1424d668a`, after the initializer `0x1424d7840`
/// sets `+0x08` from `[rdi+0x5c]` and `+0x10` to 1.
pub fn block_read_stored(
    state: &mut [u8],
    dst: &mut [u8],
    dst_len: u32,
    src: &[u8],
    src_len: u32,
    out_decoded: &mut u32,
    out_consumed: &mut u32,
) -> u8 {
    *out_decoded = 0;
    *out_consumed = 0;
    // `test rdx, rdx` / `test r9, r9` (status 3 for a null pointer) is not
    // modelled: the probe's empty buffer is a non-null arena address. The
    // length edges below are the expressible part of the same checks.
    let mut pos = 0usize;
    if state[0x10] != 0 {
        if src_len < 1 {
            return 0;
        }
        let mode = src[0];
        pos = 1;
        if mode != b'N' {
            return 8;
        }
        state[0x10] = 0;
        *out_consumed += 1;
    }
    if dst_len == 0 {
        return 0;
    }
    let remaining = src_len as usize - pos;
    if remaining == 0 {
        return 0;
    }
    let len = remaining.min(dst_len as usize);
    dst[..len].copy_from_slice(&src[pos..pos + len]);
    *out_decoded += len as u32;
    *out_consumed += len as u32;
    let total = u32::from_le_bytes(state[0xc..0x10].try_into().unwrap()).wrapping_add(len as u32);
    state[0xc..0x10].copy_from_slice(&total.to_le_bytes());
    0
}

/// `BlteBlockReadLz4` (`0x1424d6e50`) with the whole chunk in memory.
///
/// `block` includes the mode byte. The client's reader parses one 11-byte
/// header, then feeds LZ4 up to `block_size + 0x10 + block_size/0xff`
/// compressed bytes per slice, repeating until the stored bytes are consumed.
/// A decode that returns zero or more than `block_size` is status 8; a short
/// slice is need-more (status 0 with nothing consumed), which is an error when
/// the caller already holds the complete chunk.
fn decode_lz4_chunk(block: &[u8], expected: Option<usize>) -> Result<Vec<u8>> {
    if block.len() < 11 {
        return Err(Error::Malformed("truncated LZ4 header"));
    }
    if block[0] != b'4' {
        return Err(Error::Malformed("LZ4 block does not start with mode 4"));
    }
    if block[1] != 1 {
        return Err(Error::Malformed("unsupported LZ4 block version"));
    }
    let declared = u64::from_be_bytes([
        block[2], block[3], block[4], block[5], block[6], block[7], block[8], block[9],
    ]);
    if declared > u64::from(u32::MAX) {
        return Err(Error::Malformed("LZ4 declared size exceeds u32"));
    }
    let exponent = block[10];
    if !(5..=16).contains(&exponent) {
        return Err(Error::Malformed("LZ4 block-size exponent outside 5..=16"));
    }
    let block_size = 1usize << exponent;
    // The client computes this in u32 and sets it to zero above 0x7e000000;
    // the exponent cap makes the large branch unreachable here.
    let max_compressed = block_size + 0x10 + block_size / 0xff;

    let mut out = Vec::new();
    let mut pos = 11usize;
    while pos < block.len() {
        let take = max_compressed.min(block.len() - pos);
        let decoded =
            lz4_decompress_safe(&block[pos..pos + take], block_size).map_err(Error::Malformed)?;
        // `BlteBlockReadLz4` requires a positive result (`0 < uVar8`).
        if decoded.is_empty() {
            return Err(Error::Malformed("LZ4 block decoded to zero bytes"));
        }
        out.extend_from_slice(&decoded);
        pos += take;
    }
    if let Some(expected) = expected {
        if out.len() != expected {
            return Err(Error::Malformed("decoded chunk size mismatch"));
        }
    }
    Ok(out)
}

/// Standard LZ4 block decompression (`FUN_1426201d0`, `LZ4_decompress_safe`).
///
/// The client's wrapper (`FUN_1426207f0`) is called with a zeroed 32-byte
/// context from `FUN_1426201c0`, so it always takes the stateless
/// `param_1[3] == 0` path into this decoder. The returned error text is ours;
/// the client only distinguishes a negative result from a positive one.
fn lz4_decompress_safe(
    src: &[u8],
    max_output: usize,
) -> std::result::Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        let token = *src.get(pos).ok_or("truncated LZ4 token")?;
        pos += 1;

        let mut literal_len = usize::from(token >> 4);
        if literal_len == 15 {
            loop {
                let byte = *src.get(pos).ok_or("truncated LZ4 literal length")?;
                pos += 1;
                literal_len = literal_len
                    .checked_add(usize::from(byte))
                    .ok_or("LZ4 literal length overflow")?;
                if byte != 0xff {
                    break;
                }
            }
        }
        if literal_len > src.len() - pos {
            return Err("truncated LZ4 literals");
        }
        if literal_len > max_output - out.len() {
            return Err("LZ4 output exceeds block size");
        }
        out.extend_from_slice(&src[pos..pos + literal_len]);
        pos += literal_len;

        // The last sequence contains literals only; the block ends there.
        if pos == src.len() {
            break;
        }

        if src.len() - pos < 2 {
            return Err("truncated LZ4 match offset");
        }
        let offset = usize::from(u16::from_le_bytes([src[pos], src[pos + 1]]));
        pos += 2;
        if offset == 0 || offset > out.len() {
            return Err("invalid LZ4 match offset");
        }

        let mut match_len = usize::from(token & 0x0f);
        if match_len == 15 {
            loop {
                let byte = *src.get(pos).ok_or("truncated LZ4 match length")?;
                pos += 1;
                match_len = match_len
                    .checked_add(usize::from(byte))
                    .ok_or("LZ4 match length overflow")?;
                if byte != 0xff {
                    break;
                }
            }
        }
        match_len = match_len
            .checked_add(4)
            .ok_or("LZ4 match length overflow")?;
        if match_len > max_output - out.len() {
            return Err("LZ4 output exceeds block size");
        }
        let start = out.len() - offset;
        for index in 0..match_len {
            let byte = out[start + index];
            out.push(byte);
        }
    }
    Ok(out)
}

fn decrypt_chunk(
    raw: &[u8],
    chunk_index: usize,
    keyring: &BTreeMap<u64, [u8; 16]>,
) -> Result<Vec<u8>> {
    let body = &raw[1..];
    let mut p = 0usize;

    let key_name_len = *body.first().ok_or(Error::Malformed("truncated E block"))? as usize;
    p += 1;
    if key_name_len != 8 {
        return Err(Error::Malformed("encrypted chunk key name is not 8 bytes"));
    }
    let key_name = body
        .get(p..p + key_name_len)
        .ok_or(Error::Malformed("truncated E block key name"))?;
    let mut id = [0u8; 8];
    id.copy_from_slice(key_name);
    let key_id = u64::from_le_bytes(id);
    p += key_name_len;

    let iv_len = *body.get(p).ok_or(Error::Malformed("truncated iv length"))? as usize;
    p += 1;
    // The client accepts any IV up to 8 bytes (`FUN_1424d61b0` rejects only
    // `8 < len`), zero-extending it into the 8-byte nonce. Builds in the wild
    // use 4 or 8; accepting the whole range is the matching behaviour.
    if iv_len > 8 {
        return Err(Error::Malformed(
            "encrypted chunk iv is longer than 8 bytes",
        ));
    }
    let iv_slice = body
        .get(p..p + iv_len)
        .ok_or(Error::Malformed("truncated iv"))?;
    // A 4-byte IV is zero-extended to the cipher's 8.
    let mut iv = [0u8; 8];
    iv[..iv_len].copy_from_slice(iv_slice);
    p += iv_len;

    let enc_type = *body
        .get(p)
        .ok_or(Error::Malformed("truncated encryption type"))?;
    p += 1;
    if enc_type != b'S' {
        return Err(Error::Malformed("unsupported BLTE encryption type"));
    }

    // Bind the ciphertext to its position so equal chunks differ on disk: the
    // block ordinal is XORed into the low word of the IV.
    let ordinal = chunk_index as u32;
    let low = u32::from_le_bytes([iv[0], iv[1], iv[2], iv[3]]) ^ ordinal;
    iv[0..4].copy_from_slice(&low.to_le_bytes());

    let key = keyring.get(&key_id).ok_or(Error::MissingKey(key_id))?;
    Ok(salsa20(key, &iv, &body[p..]))
}

fn be_u32(data: &[u8], at: usize) -> Result<u32> {
    let slice = data
        .get(at..at + 4)
        .ok_or(Error::Malformed("truncated u32"))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// The round rotation counts.
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// The round addition constants (`floor(2^32 * abs(sin(i + 1)))`).
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// The client's 64-round MD5 block transform (`Md5Transform`, `0x1426ab530`),
/// operation for operation.
///
/// `state` is the four-word chaining state and `block` the 64-byte message
/// block, both little-endian. The client loads the four state words, runs the
/// 64 rounds inlined (no loop, no conditional branch anywhere in its 1913-byte
/// body), adds each accumulator back into the state word it came from and
/// returns; the block is read only. The state is therefore the whole output,
/// and a wrong round constant, rotation, message-word index or round function
/// changes it.
pub fn md5_transform(state: &mut [u32; 4], block: &[u8]) {
    let mut words = [0u32; 16];
    for (index, word) in words.iter_mut().enumerate() {
        *word = u32::from_le_bytes([
            block[index * 4],
            block[index * 4 + 1],
            block[index * 4 + 2],
            block[index * 4 + 3],
        ]);
    }
    let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
    for index in 0..64 {
        let (f, g) = match index {
            0..=15 => ((b & c) | (!b & d), index),
            16..=31 => ((d & b) | (!d & c), (5 * index + 1) % 16),
            32..=47 => (b ^ c ^ d, (3 * index + 5) % 16),
            _ => (c ^ (b | !d), (7 * index) % 16),
        };
        let next = b.wrapping_add(
            a.wrapping_add(f)
                .wrapping_add(MD5_K[index])
                .wrapping_add(words[g])
                .rotate_left(MD5_S[index]),
        );
        a = d;
        d = c;
        c = b;
        b = next;
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

/// The client's MD5 context: the argument `Md5Init` (`0x1426a5100`),
/// `Md5Update` (`0x1426a4f40`) and `Md5Final` (`0x1426a5010`) read and write.
///
/// The layout is the three functions' own offsets, not a guess:
///
/// * `+0x00` an eight-byte message byte count. `Md5Init` writes it with one
///   `mov qword ptr [rcx], 0`; `Md5Update` adds the argument length to it and
///   recovers the buffer fill as `(count as u32) & 0x3f`, so the high half
///   only matters past 4 GiB.
/// * `+0x08` the four chaining words, little-endian, written by four 4-byte
///   stores in `Md5Init` and read by `Md5Transform`.
/// * `+0x18` the 64-byte input buffer. `Md5Update` copies into it at
///   `buf + fill`; `Md5Final` puts the `0x80` byte and the zero padding in it
///   and writes the 64-bit message length over its last eight bytes
///   (`+0x50..+0x58`).
///
/// The whole context is therefore 0x58 bytes. Neither function bounds-checks
/// anything, so a context shorter than this is a caller bug, not a case the
/// client handles.
pub const MD5_CTX_SIZE: usize = 0x58;

/// The four chaining words at `ctx + 0x08`.
fn md5_state(ctx: &[u8]) -> [u32; 4] {
    let mut state = [0u32; 4];
    for (index, word) in state.iter_mut().enumerate() {
        *word = u32::from_le_bytes(ctx[8 + index * 4..12 + index * 4].try_into().unwrap());
    }
    state
}

/// Store the four chaining words back at `ctx + 0x08`.
fn md5_write_state(ctx: &mut [u8], state: [u32; 4]) {
    for (index, word) in state.iter().enumerate() {
        ctx[8 + index * 4..12 + index * 4].copy_from_slice(&word.to_le_bytes());
    }
}

/// Transform one block held *in* the context's own buffer (`ctx + 0x18`).
///
/// `Md5Update` and `Md5Final` both pass that buffer straight to
/// `Md5Transform`, so the split keeps the state and the block in one context
/// without copying the block.
fn md5_absorb_context(ctx: &mut [u8]) {
    let (head, tail) = ctx.split_at_mut(0x18);
    let mut state = md5_state(head);
    md5_transform(&mut state, &tail[..0x40]);
    md5_write_state(head, state);
}

/// The client's MD5 state initialiser (`Md5Init`, `0x1426a5100`).
///
/// One 8-byte zero store at `+0x00` (`mov qword ptr [rcx], 0` at
/// `0x1426a511c`) and the four RFC 1321 words at `+0x08..+0x18` as four
/// `mov dword ptr [rcx+disp], imm32` stores; nothing else in the context is
/// touched, so the buffer is left holding whatever the caller had there until
/// `Md5Update` fills it. The offsets are written base-plus-displacement
/// because that is how the instructions encode them.
pub fn md5_init(ctx: &mut [u8]) {
    ctx[0..8].fill(0);
    let base = 8;
    ctx[base..base + 4].copy_from_slice(&0x6745_2301u32.to_le_bytes());
    ctx[base + 4..base + 8].copy_from_slice(&0xefcd_ab89u32.to_le_bytes());
    ctx[base + 8..base + 12].copy_from_slice(&0x98ba_dcfeu32.to_le_bytes());
    ctx[base + 12..base + 16].copy_from_slice(&0x1032_5476u32.to_le_bytes());
}

/// The client's MD5 input buffer (`Md5Update`, `0x1426a4f40`).
///
/// The client's shape, operation for operation:
///
/// 1. `fill = (count as u32) & 0x3f` is read **before** the count is advanced,
///    and the count is advanced by the whole argument length first.
/// 2. If the buffer already holds bytes and the input covers the rest of the
///    block, the block is completed (a `memcpy` at `0x1426a4f93`), transformed
///    (`0x1426a4fa0`) and the input pointer moves past those bytes.
/// 3. Every whole block left is transformed **straight from the input** (no
///    copy; `0x1426a4fc9` inside a `do { } while (--blocks)` loop), so the
///    input buffer is only ever read.
/// 4. The remainder, if any, is copied to `buf + fill` (`0x1426a4fed`).
///
/// The `len == 0` case therefore touches nothing at all, and a call whose
/// length lands exactly on the fill boundary transforms and leaves the buffer
/// empty.
pub fn md5_update(ctx: &mut [u8], data: &[u8]) {
    let count = u64::from_le_bytes(ctx[0..8].try_into().unwrap());
    let mut used = (count as u32 as usize) & 0x3f;
    let total = count.wrapping_add(data.len() as u64);
    ctx[0..8].copy_from_slice(&total.to_le_bytes());

    let mut rest = data;
    let space = 0x40 - used;
    if used != 0 && rest.len() >= space {
        ctx[0x18 + used..0x18 + 0x40].copy_from_slice(&rest[..space]);
        md5_absorb_context(ctx);
        rest = &rest[space..];
        used = 0;
    }
    while rest.len() >= 0x40 {
        let mut state = md5_state(ctx);
        md5_transform(&mut state, &rest[..0x40]);
        md5_write_state(ctx, state);
        rest = &rest[0x40..];
    }
    if !rest.is_empty() {
        ctx[0x18 + used..0x18 + used + rest.len()].copy_from_slice(rest);
    }
}

/// The client's MD5 padding and digest (`Md5Final`, `0x1426a5010`).
///
/// The `0x80` terminator goes at `buf + fill`; `filled = fill + 1` then picks
/// the padding shape:
///
/// * `filled <= 0x38`: zero up to `0x38` (nothing when `filled == 0x38`,
///   which is the client's `jae 0x1426a50ac` falling past its `memset`) and
///   transform once.
/// * otherwise: zero up to `0x40` (nothing when `filled == 0x40`, the
///   `jae 0x1426a5068` edge), transform, zero the whole `0x38` bytes and
///   transform again.
///
/// The message length is then written as `count << 3` over the buffer's last
/// eight bytes (`+0x50`) and the final transform's four words are copied out
/// little-endian as the 16-byte digest.
pub fn md5_final(ctx: &mut [u8], digest: &mut [u8]) {
    let count = u64::from_le_bytes(ctx[0..8].try_into().unwrap());
    let used = (count as u32 as usize) & 0x3f;
    let filled = used + 1;
    ctx[0x18 + used] = 0x80;
    if filled <= 0x38 {
        if filled < 0x38 {
            ctx[0x18 + filled..0x18 + 0x38].fill(0);
        }
    } else {
        if filled < 0x40 {
            ctx[0x18 + filled..0x18 + 0x40].fill(0);
        }
        md5_absorb_context(ctx);
        ctx[0x18..0x18 + 0x38].fill(0);
    }
    ctx[0x50..0x58].copy_from_slice(&count.wrapping_shl(3).to_le_bytes());
    md5_absorb_context(ctx);
    digest[..16].copy_from_slice(&ctx[8..0x18]);
}

/// Standard MD5, the client's frame-header hash, as the client composes it:
/// `Md5Init` -> `Md5Update` -> `Md5Final` over the same 0x58-byte context.
fn md5(data: &[u8]) -> [u8; 16] {
    let mut ctx = [0u8; MD5_CTX_SIZE];
    md5_init(&mut ctx);
    md5_update(&mut ctx, data);
    let mut digest = [0u8; 16];
    md5_final(&mut ctx, &mut digest);
    digest
}

/// Salsa20 in the BLTE configuration.
///
/// Two details are easy to get wrong and both are load-bearing:
///
/// * The full 20-round cipher is used, not the 8-round variant the name alone
///   might suggest.
/// * BLTE keys are 16 bytes, so the state uses the `expand 16-byte k` constants
///   and the key is placed at both key word positions, rather than the
///   32-byte layout.
///
/// A wrong variant still produces bytes; it just produces the wrong bytes, and
/// the only symptom is a chunk whose mode byte is not `N` or `Z`.
pub fn salsa20(key: &[u8; 16], nonce: &[u8; 8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 64);
    for (block_index, block) in data.chunks(64).enumerate() {
        let state = salsa20_core_state(key, nonce, block_index as u64);

        let mut x = state;
        // 20 rounds: ten column/row double rounds.
        for _ in 0..10 {
            quarter(&mut x, 0, 4, 8, 12);
            quarter(&mut x, 5, 9, 13, 1);
            quarter(&mut x, 10, 14, 2, 6);
            quarter(&mut x, 15, 3, 7, 11);
            quarter(&mut x, 0, 1, 2, 3);
            quarter(&mut x, 5, 6, 7, 4);
            quarter(&mut x, 10, 11, 8, 9);
            quarter(&mut x, 15, 12, 13, 14);
        }

        for (i, byte) in block.iter().enumerate() {
            let word_index = i / 4;
            let shift = (i % 4) * 8;
            let keystream = x[word_index].wrapping_add(state[word_index]) >> shift;
            out.push(byte ^ (keystream & 0xFF) as u8);
        }
    }
    out
}

/// `Salsa20StateInit` (`Overwatch.exe` `0x1424e3230`).
///
/// Fills the 64-byte state from a 16-byte key, an 8-byte nonce and a 64-bit
/// block counter. The client stores the sixteen words in its diagonal order,
/// which is not the order the quarter rounds index: words 0..3 are the four
/// `expand 16-byte k` constants, the key words sit at 5, 4, 9 and 12, the
/// nonce at 6 and 11, and the counter at 8 (low) and 13 (high).
///
/// `salsa20_core_state` maps this stored form onto the order the cipher below
/// uses; the BLTE path calls both for every 64-byte block, so the layout this
/// function writes is the layout the decryption consumes.
pub fn salsa20_state_init(key: &[u8; 16], nonce: &[u8; 8], counter: u64) -> [u32; 16] {
    const TAU: &[u8; 16] = b"expand 16-byte k";
    let word = |b: &[u8], i: usize| {
        u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
    };
    let mut state = [0u32; 16];
    state[0] = word(TAU, 0);
    state[1] = word(TAU, 1);
    state[2] = word(TAU, 2);
    state[3] = word(TAU, 3);
    state[4] = word(key, 1);
    state[5] = word(key, 0);
    state[6] = word(nonce, 0);
    state[7] = word(key, 0);
    state[8] = counter as u32;
    state[9] = word(key, 2);
    state[10] = word(key, 1);
    state[11] = word(nonce, 1);
    state[12] = word(key, 3);
    state[13] = (counter >> 32) as u32;
    state[14] = word(key, 3);
    state[15] = word(key, 2);
    state
}

/// The state `salsa20_state_init` writes, in the word order the quarter
/// rounds index.
///
/// The client's block loops read the stored diagonal order directly; the
/// single mapping here is that same permutation, so both spellings describe
/// one state.
fn salsa20_core_state(key: &[u8; 16], nonce: &[u8; 8], counter: u64) -> [u32; 16] {
    let s = salsa20_state_init(key, nonce, counter);
    [
        s[0], s[5], s[10], s[15], s[12], s[1], s[6], s[11], s[8], s[13], s[2], s[7], s[4], s[9],
        s[14], s[3],
    ]
}

#[inline]
fn quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[b] ^= x[a].wrapping_add(x[d]).rotate_left(7);
    x[c] ^= x[b].wrapping_add(x[a]).rotate_left(9);
    x[d] ^= x[c].wrapping_add(x[b]).rotate_left(13);
    x[a] ^= x[d].wrapping_add(x[c]).rotate_left(18);
}

/// Standard MD5 over an arbitrary byte slice.
///
/// The client's frame-header hash (`Md5Init` `0x1426a5100`, `Md5Update`
/// `0x1426a4f40`, `Md5Final` `0x1426a5010`, `Md5Transform` `0x1426ab530`) is
/// the same digest as the 16-byte page hash stored after each first key in an
/// encoding page index: measured on all 26,544 pages of the installed file.
/// The client function that checks an encoding page's hash has not been read,
/// so `EncodingFile` does not reject a mismatched page.
pub fn md5_digest(data: &[u8]) -> [u8; 16] {
    md5(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// RFC 1321's test suite. The 80-byte case crosses a block boundary; the
    /// 62-byte case exercises a two-block padding tail.
    #[test]
    fn md5_matches_rfc_1321_vectors() {
        let cases: [(&[u8], &str); 7] = [
            (b"", "d41d8cd98f00b204e9800998ecf8427e"),
            (b"a", "0cc175b9c0f1b6a831c399e269772661"),
            (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
            (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(hex(&md5(input)), expected);
        }
    }

    /// The three client entry points are the shipped path: splitting the same
    /// message at every boundary must produce the one-shot digest, because
    /// the client streams container data through exactly this context.
    #[test]
    fn md5_streaming_matches_one_shot() {
        let messages: [&[u8]; 6] = [
            b"",
            b"abc",
            b"message digest",
            b"abcdefghijklmnopqrstuvwxyz",
            b"012345678901234567890123456789012345678901234567890123456789012345678901234567890",
            b"The quick brown fox jumps over the lazy dog",
        ];
        for message in messages {
            let expected = md5(message);
            for split in 0..=message.len() {
                let mut ctx = [0u8; MD5_CTX_SIZE];
                md5_init(&mut ctx);
                md5_update(&mut ctx, &message[..split]);
                md5_update(&mut ctx, &message[split..]);
                let mut digest = [0u8; 16];
                md5_final(&mut ctx, &mut digest);
                assert_eq!(digest, expected, "split at {split} of {message:?}");
            }
        }
    }

    /// The context offsets `Md5Update` and `Md5Final` depend on: an 8-byte
    /// count at `+0x00`, the four RFC words at `+0x08`, and a buffer at
    /// `+0x18` that `Md5Init` does not touch.
    #[test]
    fn md5_context_layout_is_the_clients() {
        let mut ctx = [0xA5u8; MD5_CTX_SIZE];
        md5_init(&mut ctx);
        assert_eq!(&ctx[0..8], &[0u8; 8]);
        assert_eq!(ctx[8..12], 0x6745_2301u32.to_le_bytes());
        assert_eq!(ctx[12..16], 0xefcd_ab89u32.to_le_bytes());
        assert_eq!(ctx[16..20], 0x98ba_dcfeu32.to_le_bytes());
        assert_eq!(ctx[20..24], 0x1032_5476u32.to_le_bytes());
        assert!(ctx[0x18..].iter().all(|byte| *byte == 0xA5));

        // One buffered byte leaves the count at 1 and the byte at buf+0.
        md5_update(&mut ctx, b"x");
        assert_eq!(u64::from_le_bytes(ctx[0..8].try_into().unwrap()), 1);
        assert_eq!(ctx[0x18], b'x');

        // The padding terminator and the bit length are at the client's own
        // offsets: `0x80` after the byte, `count << 3` over buf[0x38..0x40].
        let mut digest = [0u8; 16];
        md5_final(&mut ctx, &mut digest);
        assert_eq!(u64::from_le_bytes(ctx[0x50..0x58].try_into().unwrap()), 8);
        assert_eq!(hex(&digest), "9dd4e461268c8034f5c8564e155c67a6");
    }

    fn build_frame(frame_type: u8, records: &[FrameChunk]) -> Vec<u8> {
        let record_size = if frame_type == FRAME_TYPE_EXTENDED {
            40
        } else {
            24
        };
        let header_size = 12 + record_size * records.len();
        let mut out = Vec::with_capacity(header_size);
        out.extend_from_slice(&BLTE_MAGIC);
        out.extend_from_slice(&(header_size as u32).to_be_bytes());
        out.push(frame_type);
        out.extend_from_slice(&(records.len() as u32).to_be_bytes()[1..]);
        for record in records {
            out.extend_from_slice(&record.stored.to_be_bytes());
            out.extend_from_slice(&record.decoded.to_be_bytes());
            out.extend_from_slice(&record.md5);
            if frame_type == FRAME_TYPE_EXTENDED {
                out.extend_from_slice(&record.extra.unwrap_or([0u8; 16]));
            }
        }
        assert_eq!(out.len(), header_size);
        out
    }

    fn record(stored: u32, decoded: u32, fill: u8) -> FrameChunk {
        FrameChunk {
            stored,
            decoded,
            md5: [fill; 16],
            extra: None,
        }
    }

    #[test]
    fn parses_a_normal_frame_with_the_client_size_and_records() {
        let records = [
            record(7, 5, 0x11),
            FrameChunk {
                stored: 0x1122_3344,
                decoded: 0x5566_7788,
                md5: core::array::from_fn(|i| i as u8),
                extra: None,
            },
        ];
        let data = build_frame(FRAME_TYPE_NORMAL, &records);
        let parsed = parse_frame_header(&data, None).expect("valid frame");
        assert_eq!(parsed.header_size, 12 + 24 * 2);
        assert_eq!(parsed.frame_type, FRAME_TYPE_NORMAL);
        assert_eq!(parsed.hash, md5(&data));
        assert_eq!(parsed.chunks, records);
    }

    #[test]
    fn parses_a_type_0x10_frame_and_its_parallel_extras() {
        let records = [
            FrameChunk {
                stored: 100,
                decoded: 200,
                md5: [0x21; 16],
                extra: Some([0x31; 16]),
            },
            FrameChunk {
                stored: 101,
                decoded: 201,
                md5: [0x22; 16],
                extra: Some([0x32; 16]),
            },
        ];
        let data = build_frame(FRAME_TYPE_EXTENDED, &records);
        let parsed = parse_frame_header(&data, None).expect("valid extended frame");
        assert_eq!(parsed.header_size, 12 + 40 * 2);
        assert_eq!(parsed.frame_type, FRAME_TYPE_EXTENDED);
        assert_eq!(parsed.chunks, records);
        assert_eq!(parsed.hash, md5(&data));
    }

    #[test]
    fn a_short_buffer_reports_the_client_need_more_size() {
        let records = [record(1, 1, 0), record(2, 2, 0)];
        let data = build_frame(FRAME_TYPE_NORMAL, &records); // 60-byte header

        assert_eq!(
            parse_frame_header(&data[..35], None),
            Err(FrameHeaderError::NeedMore { needed: 0x24 })
        );
        // The 36-byte prefix passes the field checks but is shorter than the
        // declared 60-byte header, so the client reports the header size.
        assert_eq!(
            parse_frame_header(&data[..36], None),
            Err(FrameHeaderError::NeedMore { needed: 60 })
        );
    }

    #[test]
    fn rejects_each_header_field_the_client_rejects() {
        let data = build_frame(FRAME_TYPE_NORMAL, &[record(1, 1, 0)]);

        let mut wrong_magic = data.clone();
        wrong_magic[0] = b'X';
        assert!(matches!(
            parse_frame_header(&wrong_magic, None),
            Err(FrameHeaderError::Invalid("bad BLTE magic"))
        ));

        let mut zero_size = data.clone();
        zero_size[4..8].copy_from_slice(&0u32.to_be_bytes());
        assert!(matches!(
            parse_frame_header(&zero_size, None),
            Err(FrameHeaderError::Invalid("zero frame-header size"))
        ));

        let mut wrong_type = data.clone();
        wrong_type[8] = 0x0e;
        assert!(matches!(
            parse_frame_header(&wrong_type, None),
            Err(FrameHeaderError::Invalid("unsupported frame type"))
        ));

        let mut zero_count = data.clone();
        zero_count[9..12].copy_from_slice(&[0, 0, 0]);
        assert!(matches!(
            parse_frame_header(&zero_count, None),
            Err(FrameHeaderError::Invalid("zero chunk count"))
        ));

        let mut wrong_size = data.clone();
        wrong_size[4..8].copy_from_slice(&(data.len() as u32 + 1).to_be_bytes());
        assert!(matches!(
            parse_frame_header(&wrong_size, None),
            Err(FrameHeaderError::Invalid("frame-header size mismatch"))
        ));
    }

    #[test]
    fn a_type_0x10_record_cannot_use_the_normal_stride() {
        let data = build_frame(FRAME_TYPE_NORMAL, &[record(1, 1, 0)]);
        let mut typed = data.clone();
        typed[8] = FRAME_TYPE_EXTENDED;
        assert!(matches!(
            parse_frame_header(&typed, None),
            Err(FrameHeaderError::Invalid("frame-header size mismatch"))
        ));
    }

    #[test]
    fn the_optional_hash_check_compares_only_the_given_length() {
        let data = build_frame(FRAME_TYPE_NORMAL, &[record(1, 1, 0)]);
        let hash = md5(&data);
        assert!(parse_frame_header(&data, Some(&hash)).is_ok());
        assert!(parse_frame_header(&data, Some(&hash[..4])).is_ok());

        let mut wrong = hash;
        wrong[0] ^= 1;
        assert!(matches!(
            parse_frame_header(&data, Some(&wrong)),
            Err(FrameHeaderError::Invalid("frame-header hash mismatch"))
        ));
        assert!(matches!(
            parse_frame_header(&data, Some(&hash[..4])).map(|_| ()),
            Ok(())
        ));
        let mut short_wrong = hash[..4].to_vec();
        short_wrong[0] ^= 1;
        assert!(matches!(
            parse_frame_header(&data, Some(&short_wrong)),
            Err(FrameHeaderError::Invalid("frame-header hash mismatch"))
        ));
        assert!(matches!(
            parse_frame_header(&data, Some(&[0u8; 17])),
            Err(FrameHeaderError::Invalid(
                "expected hash longer than 16 bytes"
            ))
        ));
    }

    #[test]
    fn the_hash_covers_exactly_the_declared_header() {
        let data = build_frame(FRAME_TYPE_NORMAL, &[record(1, 1, 0)]);
        let header_hash = md5(&data);
        let mut with_trailing = data.clone();
        with_trailing.extend_from_slice(b"trailing bytes are not hashed");
        let parsed = parse_frame_header(&with_trailing, Some(&header_hash)).expect("valid frame");
        assert_eq!(parsed.header_size, data.len());
        assert_eq!(parsed.hash, header_hash);
    }

    #[test]
    fn a_header_only_record_decodes_to_no_bytes() {
        let mut record = vec![0u8; DATA_HEADER_SIZE];
        record[16..20].copy_from_slice(&(DATA_HEADER_SIZE as u32).to_le_bytes());
        assert_eq!(decode(&record, &BTreeMap::new()).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn a_direct_container_with_no_frame_header_keeps_its_single_chunk() {
        let mut data = b"BLTE\0\0\0\0".to_vec();
        data.extend_from_slice(b"Ndirect");
        assert_eq!(decode(&data, &BTreeMap::new()).unwrap(), b"direct".to_vec());
    }

    /// Multi-chunk layout: the frame header is 12 bytes plus 24 per chunk and
    /// the chunks start immediately after it. There is no hash between the
    /// table and the first chunk; the client's parser expects
    /// `header_size == 12 + 24 * count` and reads chunk 0 at `header_size`.
    #[test]
    fn multi_chunk_container_starts_chunks_at_the_frame_header_size() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let plain: [&[u8]; 2] = [b"hello", b"world"];
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(plain[1]).unwrap();
        let zbody = z.finish().unwrap();

        let chunk0 = {
            let mut c = vec![b'N'];
            c.extend_from_slice(plain[0]);
            c
        };
        let chunk1 = {
            let mut c = vec![b'Z'];
            c.extend_from_slice(&zbody);
            c
        };
        let chunks = [&chunk0, &chunk1];

        let header_size = 12 + 24 * chunks.len();
        let mut blte = Vec::new();
        blte.extend_from_slice(b"BLTE");
        blte.extend_from_slice(&(header_size as u32).to_be_bytes());
        blte.push(0x0F);
        blte.extend_from_slice(&[0, 0, chunks.len() as u8]);
        for (i, chunk) in chunks.iter().enumerate() {
            blte.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
            blte.extend_from_slice(&(plain[i].len() as u32).to_be_bytes());
            blte.extend_from_slice(&[0u8; 16]); // per-chunk hash, not checked
        }
        assert_eq!(blte.len(), header_size);
        for chunk in &chunks {
            blte.extend_from_slice(chunk);
        }

        let mut record = vec![0u8; DATA_HEADER_SIZE];
        record[16..20].copy_from_slice(&(blte.len() as u32 + 30).to_le_bytes());
        record.extend_from_slice(&blte);

        assert_eq!(decode(&record, &BTreeMap::new()).unwrap(), b"helloworld");
    }

    /// Type `0x10` records are 40 bytes; the extra 16 bytes sit after the
    /// chunk MD5 and do not move the first chunk, which still starts at the
    /// declared header size.
    #[test]
    fn a_type_0x10_container_decodes_from_the_40_byte_stride() {
        let plain = b"extended records";
        let chunk = {
            let mut c = vec![b'N'];
            c.extend_from_slice(plain);
            c
        };
        let header_size = 12 + 40;
        let mut blte = Vec::new();
        blte.extend_from_slice(b"BLTE");
        blte.extend_from_slice(&(header_size as u32).to_be_bytes());
        blte.push(FRAME_TYPE_EXTENDED);
        blte.extend_from_slice(&[0, 0, 1]);
        blte.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
        blte.extend_from_slice(&(plain.len() as u32).to_be_bytes());
        blte.extend_from_slice(&[0u8; 16]);
        blte.extend_from_slice(&[0xAB; 16]);
        assert_eq!(blte.len(), header_size);
        blte.extend_from_slice(&chunk);

        assert_eq!(decode(&blte, &BTreeMap::new()).unwrap(), plain);
    }

    /// A literals-only LZ4 block. The reference decoder accepts this shape;
    /// it is enough to prove the `4` header and the slice loop.
    fn lz4_literals(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = data.len();
        if rest < 15 {
            out.push((rest as u8) << 4);
        } else {
            out.push(0xF0);
            rest -= 15;
            while rest >= 255 {
                out.push(255);
                rest -= 255;
            }
            out.push(rest as u8);
        }
        out.extend_from_slice(data);
        out
    }

    fn lz4_chunk(data: &[u8], exponent: u8) -> Vec<u8> {
        let compressed = lz4_literals(data);
        let mut out = vec![b'4', 1];
        out.extend_from_slice(&(compressed.len() as u64).to_be_bytes());
        out.push(exponent);
        out.extend_from_slice(&compressed);
        out
    }

    fn container(frame_type: u8, chunks: &[&[u8]], decoded: &[usize]) -> Vec<u8> {
        let record_size = if frame_type == FRAME_TYPE_EXTENDED {
            40
        } else {
            24
        };
        let header_size = 12 + record_size * chunks.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"BLTE");
        out.extend_from_slice(&(header_size as u32).to_be_bytes());
        out.push(frame_type);
        out.extend_from_slice(&(chunks.len() as u32).to_be_bytes()[1..]);
        for (chunk, decoded) in chunks.iter().zip(decoded) {
            out.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
            out.extend_from_slice(&(*decoded as u32).to_be_bytes());
            out.extend_from_slice(&[0u8; 16]);
            if frame_type == FRAME_TYPE_EXTENDED {
                out.extend_from_slice(&[0xCD; 16]);
            }
        }
        assert_eq!(out.len(), header_size);
        for chunk in chunks {
            out.extend_from_slice(chunk);
        }
        out
    }

    #[test]
    fn an_lz4_chunk_decodes_through_the_client_header() {
        let plain = b"the quick brown fox jumps over the lazy dog";
        let chunk = lz4_chunk(plain, 9);
        let data = container(FRAME_TYPE_NORMAL, &[&chunk], &[plain.len()]);
        assert_eq!(decode(&data, &BTreeMap::new()).unwrap(), plain);
    }

    #[test]
    fn an_encrypted_lz4_chunk_dispatches_the_inner_mode() {
        let key = [0x5Au8; 16];
        let key_name = *b"LZ4KEY!!";
        let key_id = u64::from_le_bytes(key_name);
        let keyring = BTreeMap::from([(key_id, key)]);
        let plain = b"encrypted lz4 payload";
        let inner = lz4_chunk(plain, 9);

        let iv = [0x12u8, 0x34, 0x56, 0x78];
        let mut nonce = [0u8; 8];
        nonce[..iv.len()].copy_from_slice(&iv);
        let cipher = salsa20(&key, &nonce, &inner);
        let mut chunk = vec![b'E', 8];
        chunk.extend_from_slice(&key_name);
        chunk.push(iv.len() as u8);
        chunk.extend_from_slice(&iv);
        chunk.push(b'S');
        chunk.extend_from_slice(&cipher);

        let data = container(FRAME_TYPE_NORMAL, &[&chunk], &[plain.len()]);
        assert_eq!(decode(&data, &keyring).unwrap(), plain);
    }

    #[test]
    fn lz4_header_rejections_match_the_client_checks() {
        let plain = b"payload";
        let good = lz4_chunk(plain, 9);

        let mut bad_version = good.clone();
        bad_version[1] = 2;
        let data = container(FRAME_TYPE_NORMAL, &[&bad_version], &[plain.len()]);
        assert!(decode(&data, &BTreeMap::new()).is_err());

        let mut bad_exponent = good.clone();
        bad_exponent[10] = 4;
        let data = container(FRAME_TYPE_NORMAL, &[&bad_exponent], &[plain.len()]);
        assert!(decode(&data, &BTreeMap::new()).is_err());

        let mut too_large = good.clone();
        too_large[2..10].copy_from_slice(&(u64::from(u32::MAX) + 1).to_be_bytes());
        let data = container(FRAME_TYPE_NORMAL, &[&too_large], &[plain.len()]);
        assert!(decode(&data, &BTreeMap::new()).is_err());

        let short = vec![b'4', 1, 0, 0, 0, 0, 0, 0, 0, 1];
        let data = container(FRAME_TYPE_NORMAL, &[&short], &[plain.len()]);
        assert!(decode(&data, &BTreeMap::new()).is_err());

        let malformed = vec![b'4', 1, 0, 0, 0, 0, 0, 0, 0, 1, 9, 0xF0];
        let data = container(FRAME_TYPE_NORMAL, &[&malformed], &[plain.len()]);
        assert!(decode(&data, &BTreeMap::new()).is_err());
    }

    #[test]
    fn lz4_decoded_size_must_match_the_frame_record() {
        let plain = b"size checked";
        let chunk = lz4_chunk(plain, 9);
        let data = container(FRAME_TYPE_NORMAL, &[&chunk], &[plain.len() + 1]);
        assert!(decode(&data, &BTreeMap::new()).is_err());
    }

    /// The `E` header's IV length is a byte, and the client accepts any value
    /// up to 8 (zero-extending it). Chunk `n` XORs `n` into the first IV word.
    #[test]
    fn encrypted_chunk_with_a_short_iv_uses_the_chunk_ordinal() {
        let key = [0x11u8; 16];
        let key_id = u64::from_le_bytes(*b"KEYNAME!");
        let keyring = BTreeMap::from([(key_id, key)]);

        let index = 1u32;
        let iv = [0xAAu8, 0xBB]; // 2-byte IV, zero-extended to 8
        let mut nonce = [0u8; 8];
        nonce[..2].copy_from_slice(&iv);
        let low = u32::from_le_bytes([nonce[0], nonce[1], nonce[2], nonce[3]]) ^ index;
        nonce[..4].copy_from_slice(&low.to_le_bytes());

        let plain = b"Npayload";
        let cipher = salsa20(&key, &nonce, plain);
        let mut block = vec![b'E', 8];
        block.extend_from_slice(b"KEYNAME!");
        block.push(iv.len() as u8);
        block.extend_from_slice(&iv);
        block.push(b'S');
        block.extend_from_slice(&cipher);

        // Chunk 0 is a stored block so the encrypted chunk's ordinal is 1.
        let dummy = b"Nx";
        let header_size = 12 + 24 * 2;
        let mut blte = Vec::new();
        blte.extend_from_slice(b"BLTE");
        blte.extend_from_slice(&(header_size as u32).to_be_bytes());
        blte.push(0x0F);
        blte.extend_from_slice(&[0, 0, 2]);
        // The decoded size counts the block after the mode byte is removed.
        for (stored, decoded) in [(dummy.len(), 1), (block.len(), plain.len() - 1)] {
            blte.extend_from_slice(&(stored as u32).to_be_bytes());
            blte.extend_from_slice(&(decoded as u32).to_be_bytes());
            blte.extend_from_slice(&[0u8; 16]);
        }
        blte.extend_from_slice(dummy);
        blte.extend_from_slice(&block);

        let decoded = decode(&blte, &keyring).unwrap();
        assert_eq!(decoded, b"xpayload");
    }

    /// The ECRYPT reference keystream for Salsa20 with a 128-bit key and an
    /// 8-byte nonce. This pins both choices that are easy to get wrong: the
    /// round count and the key schedule.
    #[test]
    fn matches_the_reference_keystream() {
        let key = {
            let mut k = [0u8; 16];
            k[0] = 0x80;
            k
        };
        let nonce = [0u8; 8];
        let stream = salsa20(&key, &nonce, &[0u8; 64]);
        let expected = "4dfa5e481da23ea09a31022050859936da52fcee218005164f267cb65f5cfd7f\
             2b4f97e0ff16924a52df269515110a07f9e460bc65ef95da58f740b7d1dbb0aa";
        assert_eq!(hex(&stream), expected.replace(['\n', ' '], ""));
    }

    /// The ECRYPT vector has a zero nonce and one non-zero key byte, so it
    /// does not exercise most state words. This vector is the output of the
    /// client's own `Salsa20StateInit`/`Salsa20Xor` under Ghidra's emulator
    /// with key `01..10`, nonce `10..17`, counter 0 and 128 zero bytes; it
    /// pins every key, nonce and constant word.
    #[test]
    fn matches_the_client_state_layout_with_every_word_nonzero() {
        let key: [u8; 16] = core::array::from_fn(|i| (i + 1) as u8);
        let nonce = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];
        let stream = salsa20(&key, &nonce, &[0u8; 128]);
        assert_eq!(
            hex(&stream),
            "6655b5ece02c1f27eae19661e68d39b6a9d2e2581e7dc624ff0b1ed83185e648\
             57107cd186f82f50e4fbe28881df243bb6f61c0716ca613bebbe319339c637c7\
             5c81b569d1b11156314bf040649b9ca133f8a4abc4704248def7d7ac23c27fe7\
             204577e0fa45b754944b68bfc2727e4ca88c7a26a4a82f62385b66239d1b907b"
        );
    }

    /// `Salsa20StateInit` (`0x1424e3230`) under the emulator for key
    /// `01..10`, nonce `10..17` and counter 0: the exact 64 bytes it stores.
    /// This pins the client's diagonal order as its own value, independent of
    /// the ciphertext vector above.
    #[test]
    fn state_init_matches_the_client_state_vector() {
        let key: [u8; 16] = core::array::from_fn(|i| (i + 1) as u8);
        let nonce = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];
        let state = salsa20_state_init(&key, &nonce, 0);
        let bytes: Vec<u8> = state.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(
            hex(&bytes),
            "657870616e642031362d62797465206b05060708010203041011121301020304\
             00000000090a0b0c05060708141516170d0e0f10000000000d0e0f10090a0b0c"
        );
    }

    /// The 64-bit counter is split little-endian across words 8 (low) and 13
    /// (high); both halves and the key words around them stay distinct.
    #[test]
    fn state_init_splits_the_counter_at_words_eight_and_thirteen() {
        let key: [u8; 16] = core::array::from_fn(|i| (i + 20) as u8);
        let nonce = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7];
        let state = salsa20_state_init(&key, &nonce, 0x1122_3344_5566_7788);
        assert_eq!(state[8], 0x5566_7788);
        assert_eq!(state[13], 0x1122_3344);
        assert_eq!(
            state[9],
            u32::from_le_bytes([key[8], key[9], key[10], key[11]])
        );
        assert_eq!(
            state[10],
            u32::from_le_bytes([key[4], key[5], key[6], key[7]])
        );
    }

    #[test]
    fn stripping_a_data_header_is_idempotent() {
        let bare = b"BLTE\0\0\0\0rest";
        assert_eq!(strip_data_header(bare).unwrap(), bare);

        let mut wrapped = vec![0u8; DATA_HEADER_SIZE];
        wrapped.extend_from_slice(bare);
        assert_eq!(strip_data_header(&wrapped).unwrap(), bare);
    }

    #[test]
    fn rejects_a_non_blte_payload() {
        assert!(strip_data_header(b"not a container at all").is_err());
    }
}

//! BLTE decoding, including the Salsa20-encrypted chunk mode.
//!
//! A stored payload is either raw `BLTE` or preceded by a 30-byte data header
//! (`md5[16]`, `size u32 LE`, 10 further bytes). After the magic comes a
//! big-endian frame-header size; when it is non-zero a chunk table follows
//! (`0x0F`, `u24` count, then `count` records of `enc u32 BE`, `dec u32 BE`,
//! `md5[16]`). Each chunk starts with a one-byte mode:
//!
//! * `N` stored
//! * `Z` zlib
//! * `E` encrypted: `u8 key-name-len`, key name, `u8 iv-len`, iv, `u8 'S'`,
//!   then Salsa20 ciphertext whose plaintext begins with the real mode byte.
//!
//! The chunk ordinal is folded into the first four IV bytes so identical chunks
//! do not encrypt identically.

use std::collections::BTreeMap;
use std::io::Read;

pub const BLTE_MAGIC: [u8; 4] = *b"BLTE";
/// Size of the optional header in front of the BLTE payload.
pub const DATA_HEADER_SIZE: usize = 30;

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
    let blte = strip_data_header(data)?;
    if blte.len() < 8 {
        return Err(Error::Malformed("truncated container"));
    }
    let frame_header_size = be_u32(blte, 4)? as usize;

    let (chunks, mut pos) = if frame_header_size == 0 {
        // Single implicit chunk covering the remainder; its decoded size is not
        // declared, so accept whatever the mode block yields.
        (
            vec![Chunk {
                stored: blte.len() - 8,
                decoded: None,
            }],
            8usize,
        )
    } else {
        if blte.len() < 12 || blte[8] != 0x0F {
            return Err(Error::Malformed("unexpected chunk-table header"));
        }
        let count = u24_be(blte, 9)? as usize;
        // Each record costs 24 bytes from offset 12, so a count larger than
        // what the container can hold is malformed. Cap before reserving: a
        // corrupt u24 must not allocate hundreds of MB first (ow-stu's bag
        // parsers do the same check before their reserve).
        let max_records = blte.len().saturating_sub(12) / 24;
        let mut table = Vec::with_capacity(count.min(max_records));
        let mut p = 12;
        for _ in 0..count {
            if p + 24 > blte.len() {
                return Err(Error::Malformed("truncated chunk record"));
            }
            table.push(Chunk {
                stored: be_u32(blte, p)? as usize,
                decoded: Some(be_u32(blte, p + 4)? as usize),
            });
            p += 24;
        }
        (table, p)
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
        let (mode, body) = if raw[0] == b'E' {
            decrypted = decrypt_chunk(raw, index, keyring)?;
            if decrypted.is_empty() {
                return Err(Error::Malformed("empty decrypted chunk"));
            }
            (decrypted[0], &decrypted[1..])
        } else {
            (raw[0], &raw[1..])
        };

        let block = match mode {
            b'N' => body.to_vec(),
            b'Z' => {
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
    if iv_len != 4 && iv_len != 8 {
        return Err(Error::Malformed(
            "encrypted chunk iv is neither 4 nor 8 bytes",
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

fn u24_be(data: &[u8], at: usize) -> Result<u32> {
    let slice = data
        .get(at..at + 3)
        .ok_or(Error::Malformed("truncated u24"))?;
    Ok(((slice[0] as u32) << 16) | ((slice[1] as u32) << 8) | slice[2] as u32)
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
    // 16-byte key: the `expand 16-byte k` constants, key occupying both halves.
    const TAU: &[u8; 16] = b"expand 16-byte k";
    let key_lo = key;
    let key_hi = key;

    let word = |b: &[u8], i: usize| {
        u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
    };
    let constants: &[u8] = TAU;

    let mut out = Vec::with_capacity(data.len() + 64);
    for (block_index, block) in data.chunks(64).enumerate() {
        let counter = block_index as u64;

        let mut state = [0u32; 16];
        state[0] = word(constants, 0);
        state[1] = word(key_lo, 0);
        state[2] = word(key_lo, 1);
        state[3] = word(key_lo, 2);
        state[4] = word(key_lo, 3);
        state[5] = word(constants, 1);
        state[6] = word(nonce, 0);
        state[7] = word(nonce, 1);
        state[8] = counter as u32;
        state[9] = (counter >> 32) as u32;
        state[10] = word(constants, 2);
        state[11] = word(key_hi, 0);
        state[12] = word(key_hi, 1);
        state[13] = word(key_hi, 2);
        state[14] = word(key_hi, 3);
        state[15] = word(constants, 3);

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

#[inline]
fn quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[b] ^= x[a].wrapping_add(x[d]).rotate_left(7);
    x[c] ^= x[b].wrapping_add(x[a]).rotate_left(9);
    x[d] ^= x[c].wrapping_add(x[b]).rotate_left(13);
    x[a] ^= x[d].wrapping_add(x[c]).rotate_left(18);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let got: String = stream.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(got, expected.replace(['\n', ' '], ""));
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

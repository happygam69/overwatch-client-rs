//! The bucket index: maps a 9-byte encoding key to a location in a data file.
//!
//! `data/<bucket:02X>*.idx` holds 16 buckets. Each file starts with a
//! `BlockSizeAndHash` section header (`i32` byte count, `i32` hash), a 16-byte
//! file header, padding to the 16-byte-aligned end of that section, then the
//! entry block's own `BlockSizeAndHash` and that many bytes of fixed 18-byte
//! entries:
//!
//! ```text
//! EKey[9]  | location[5, big-endian] | size u32 LE
//! ```
//!
//! The two fields disagree on endianness, which is easy to get wrong: the
//! 40-bit location splits into `archive = value >> 30` and
//! `offset = value & (2^30 - 1)` read big-endian, while `size` is a plain
//! little-endian `u32`. `size` counts the *whole* stored record, including the
//! 30-byte data header when the payload is wrapped in one.
//!
//! Files are preallocated, so a zero-padded tail follows the entries; the entry
//! count comes from the block header, never from the file size.
//!
//! Bucket selection hashes the 9 key bytes: XOR them, then fold the nibbles.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The leading `BlockSizeAndHash` section header.
pub const SECTION_HEADER: usize = 8;
/// The 16-byte file header that follows it (version, bucket, widths, ...).
pub const FILE_HEADER_SIZE: usize = 16;
pub const ENTRY_SIZE: usize = 18;
/// Where entries begin in the usual file: a 16-byte section, padding to 32,
/// then the entry block's 8-byte header. Real files can declare another
/// section size; `parse_bucket_file` computes the offset from it.
pub const ENTRIES_OFFSET: usize = 40;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Malformed(PathBuf, &'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Malformed(p, m) => write!(f, "malformed index {}: {m}", p.display()),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Where a stored record lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// Ordinal of `data/data.NNN` holding the record.
    pub archive: u32,
    /// Byte offset of the record within that archive.
    pub offset: u64,
    /// Stored length, including any 30-byte data header.
    pub size: u32,
}

/// Parse one bucket file. `expect_bucket` is verified against the header.
pub fn parse_bucket_file(
    path: &Path,
    expect_bucket: Option<u8>,
) -> Result<Vec<([u8; 9], Location)>> {
    let data = fs::read(path)?;
    if data.len() < SECTION_HEADER + FILE_HEADER_SIZE {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file shorter than its header",
        ));
    }

    // The section header's byte count positions the entry block. The client
    // (`0x14250fcf0`) requires it to be at least 0x10 and rounds
    // `sectionSize + 0x17` down to a 16-byte boundary; the entry block's own
    // 8-byte BlockSizeAndHash follows, then the entries. The usual file has
    // sectionSize 16 -> entry header at 32 -> entries at 40.
    let section_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if section_size < 0x10 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section header is smaller than 16 bytes",
        ));
    }
    let entry_header = (section_size as usize + 0x17) & !0xf;
    if data.len() < entry_header + SECTION_HEADER {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block header runs past the end of the file",
        ));
    }
    let entries_offset = entry_header + SECTION_HEADER;

    // The section's second word is a 32-bit hash of the section body, and the
    // entry block's second word is a rolling hash over the 18-byte entries.
    // The client checks both before using the segment (`0x14250fcf0`); this is
    // the same hash, verified against all sixteen files of the reference
    // install. Rejecting a mismatch is what keeps a corrupt index from
    // resolving to arbitrary bytes.
    let header_end = 8 + section_size as usize;
    if data.len() < header_end {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section body runs past the end of the file",
        ));
    }
    let mut state = [0u32; 2];
    hash_index(&data[8..header_end], &mut state);
    let stored = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if state[0] != stored {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file header hash mismatch",
        ));
    }

    let version = u16::from_le_bytes([data[8], data[9]]);
    let bucket = data[10];
    let span_size = data[12];
    let span_offs = data[13];
    let ekey_bytes = data[14];

    // The client's parser (`FUN_14250fcf0`) accepts any version >= 7 and
    // checks only the three field widths it hardcodes: 4-byte size, 5-byte
    // location, 9-byte key. It never looks at the extra-bytes or offset-bits
    // bytes, so neither do we; a v8 file with the same widths reads the same.
    if version < 7 || span_size != 4 || span_offs != 5 || ekey_bytes != 9 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "unsupported index header",
        ));
    }
    if let Some(expected) = expect_bucket {
        if bucket != expected {
            return Err(Error::Malformed(
                path.to_path_buf(),
                "bucket number disagrees with filename",
            ));
        }
    }

    let block_size = i32::from_le_bytes([
        data[entry_header],
        data[entry_header + 1],
        data[entry_header + 2],
        data[entry_header + 3],
    ]);
    if block_size < 0 || block_size as usize % ENTRY_SIZE != 0 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block size is not a multiple of 18",
        ));
    }
    let count = block_size as usize / ENTRY_SIZE;

    let end = entries_offset + count * ENTRY_SIZE;
    if data.len() < end {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entries run past the end of the file",
        ));
    }

    let mut state = [0u32; 2];
    for entry in data[entries_offset..end].chunks_exact(ENTRY_SIZE) {
        hash_index(entry, &mut state);
    }
    let stored = u32::from_le_bytes([
        data[entry_header + 4],
        data[entry_header + 5],
        data[entry_header + 6],
        data[entry_header + 7],
    ]);
    if state[0] != stored {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block hash mismatch",
        ));
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = &data[entries_offset + i * ENTRY_SIZE..entries_offset + (i + 1) * ENTRY_SIZE];
        let mut ekey = [0u8; 9];
        ekey.copy_from_slice(&e[0..9]);

        let mut packed: u64 = 0;
        for b in &e[9..14] {
            packed = (packed << 8) | u64::from(*b);
        }
        let size = u32::from_le_bytes([e[14], e[15], e[16], e[17]]);

        out.push((
            ekey,
            Location {
                archive: (packed >> 30) as u32,
                offset: packed & ((1 << 30) - 1),
                size,
            },
        ));
    }
    Ok(out)
}

/// The client's dynamic-index hash, exactly (`DynamicIndexHash`,
/// `0x142512c60`).
///
/// It is a two-word 32-bit state hash over 12-byte blocks. The same function
/// hashes the file header (state starts at zero) and each 18-byte entry in
/// turn (the state carries between entries); `state[0]` is compared with the
/// stored hash. Verified against all sixteen `.idx` files of the reference
/// install: the file-header hash and the complete entry-block rolling hash
/// both match. Public so the verification probe calls this implementation
/// rather than a copy.
pub fn hash_index(data: &[u8], state: &mut [u32; 2]) {
    let mut a = (data.len() as u32)
        .wrapping_add(0xdead_beef)
        .wrapping_add(state[0]);
    let mut b = state[1].wrapping_add(a);
    let mut c = a;

    // A zero-length segment is the client's switch case 0: it stores the two
    // running words and returns before the final mix. The parsed index never
    // hashes an empty slice, but the function's contract includes it.
    if data.is_empty() {
        state[0] = b;
        state[1] = a;
        return;
    }

    let mut pos = 0usize;
    // The client's loop runs floor((len - 1) / 12) blocks: the last 1..=12
    // bytes are the switch tail, including a whole twelve-byte group when the
    // length is a multiple of 12. The group is added to the running words, not
    // mixed as a block.
    while pos + 12 < data.len() {
        let w0 = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let w1 = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let w2 = u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
        b = b.wrapping_add(w2);
        c = b.rotate_left(4) ^ w0.wrapping_sub(b).wrapping_add(c);
        let i5 = a.wrapping_add(w1).wrapping_add(b);
        let i12 = i5.wrapping_add(c);
        c = c.rotate_left(6) ^ a.wrapping_add(w1).wrapping_sub(c);
        a = c.rotate_left(8) ^ i5.wrapping_sub(c);
        let i6 = i12.wrapping_add(c);
        c = a.rotate_left(16) ^ i12.wrapping_sub(a);
        let i5 = i6.wrapping_add(a);
        a = c.rotate_left(19) ^ i6.wrapping_sub(c);
        c = c.wrapping_add(i5);
        b = a.rotate_left(4) ^ i5.wrapping_sub(a);
        a = a.wrapping_add(c);
        pos += 12;
    }

    let tail = &data[pos..];
    match tail.len() {
        0 => unreachable!("a non-empty input leaves a 1..=12 byte tail"),
        1 => c = c.wrapping_add(u32::from(tail[0])),
        2 => c = c.wrapping_add(le(tail, 0, 2)),
        3 => c = c.wrapping_add(le(tail, 0, 3)),
        4 => c = c.wrapping_add(le(tail, 0, 4)),
        5 => {
            a = a.wrapping_add(u32::from(tail[4]));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        6 => {
            a = a.wrapping_add(le(tail, 4, 2));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        7 => {
            a = a.wrapping_add(le(tail, 4, 3));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        8 => {
            a = a.wrapping_add(le(tail, 4, 4));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        9 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(u32::from(tail[8]));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        10 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(le(tail, 8, 2));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        11 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(le(tail, 8, 3));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        12 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(le(tail, 8, 4));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        _ => unreachable!("the block loop leaves at most twelve bytes"),
    }

    b = (b ^ a).wrapping_sub(a.rotate_left(14));
    c = (c ^ b).wrapping_sub(b.rotate_left(11));
    a = (a ^ c).wrapping_sub(c.rotate_left(25));
    b = (b ^ a).wrapping_sub(a.rotate_left(16));
    c = (c ^ b).wrapping_sub(b.rotate_left(4));
    c = (c ^ a).wrapping_sub(c.rotate_left(14));
    state[0] = (b ^ c).wrapping_sub(c.rotate_left(24));
    state[1] = c;
}

fn le(data: &[u8], at: usize, len: usize) -> u32 {
    let mut value = 0u32;
    for i in 0..len {
        value |= u32::from(data[at + i]) << (8 * i);
    }
    value
}

/// The client's per-record dynamic-index key hash, exactly
/// (`DynamicIndexKeyHash`, `0x142513230`).
///
/// It is the lookup3 `hashlittle` word hash: the three running words start at
/// `0xdeadbeef + len + init` (all 32-bit wrapping adds), twelve-byte groups are
/// mixed, and the 1..=12 byte tail is added before the final mix. The returned
/// word is the final `c`. A zero-length key is the client's `case 0`: it
/// returns `c` before the final mix.
///
/// `DynamicIndexSegmentParse` (`0x14250fcf0`) calls it once per 24-byte page
/// record as `key_hash(&record[4..23], 0)`, then compares the result ORed with
/// `0x80000000` against the record's first dword. The client selects one of
/// three load paths from the key pointer's low two bits (dword, word and byte
/// loads); all three assemble the same twelve bytes, so this byte-wise
/// implementation is the function's return contract on every alignment.
///
/// A page record is 4-byte stored hash, 19-byte key, one pad byte. The install's
/// page tables are zero-filled, so this function has no stored record hash to
/// agree with; the unit tests pin the client's own outputs instead.
pub fn key_hash(key: &[u8], init: u32) -> u32 {
    use std::num::Wrapping;

    let seed = Wrapping(key.len() as u32) + Wrapping(0xdead_beef) + Wrapping(init);
    let mut a = seed;
    let mut b = seed;
    let mut c = seed;

    let mut pos = 0usize;
    while key.len() - pos > 12 {
        let w0 = Wrapping(le(key, pos, 4));
        let w1 = Wrapping(le(key, pos + 4, 4));
        let w2 = Wrapping(le(key, pos + 8, 4));
        a += w0;
        b += w1;
        c += w2;
        a = (a - c) ^ Wrapping(c.0.rotate_left(4));
        c += b;
        b = (b - a) ^ Wrapping(a.0.rotate_left(6));
        a += c;
        c = (c - b) ^ Wrapping(b.0.rotate_left(8));
        b += a;
        a = (a - c) ^ Wrapping(c.0.rotate_left(16));
        c += b;
        b = (b - a) ^ Wrapping(a.0.rotate_left(19));
        a += c;
        c = (c - b) ^ Wrapping(b.0.rotate_left(4));
        b += a;
        pos += 12;
    }

    let tail = &key[pos..];
    match tail.len() {
        0 => return c.0,
        1 => a += Wrapping(u32::from(tail[0])),
        2 => a += Wrapping(le(tail, 0, 2)),
        3 => a += Wrapping(le(tail, 0, 3)),
        4 => a += Wrapping(le(tail, 0, 4)),
        5 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(u32::from(tail[4]));
        }
        6 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 2));
        }
        7 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 3));
        }
        8 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
        }
        9 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(u32::from(tail[8]));
        }
        10 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(le(tail, 8, 2));
        }
        11 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(le(tail, 8, 3));
        }
        12 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(le(tail, 8, 4));
        }
        _ => unreachable!("the block loop leaves at most twelve bytes"),
    }

    c = (c ^ b) - Wrapping(b.0.rotate_left(14));
    a = (a ^ c) - Wrapping(c.0.rotate_left(11));
    b = (b ^ a) - Wrapping(a.0.rotate_left(25));
    c = (c ^ b) - Wrapping(b.0.rotate_left(16));
    a = (a ^ c) - Wrapping(c.0.rotate_left(4));
    b = (b ^ a) - Wrapping(a.0.rotate_left(14));
    c = (c ^ b) - Wrapping(b.0.rotate_left(24));
    c.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket_file(version: u16, bucket: u8, entries: &[([u8; 9], u64, u32)]) -> Vec<u8> {
        let mut out = vec![0u8; ENTRIES_OFFSET];
        out[0..4].copy_from_slice(&16u32.to_le_bytes());
        out[8..10].copy_from_slice(&version.to_le_bytes());
        out[10] = bucket;
        out[12] = 4;
        out[13] = 5;
        out[14] = 9;
        out[15] = 30;
        out[32..36].copy_from_slice(&((entries.len() * ENTRY_SIZE) as i32).to_le_bytes());
        for (key, packed, size) in entries {
            out.extend_from_slice(key);
            out.extend_from_slice(&packed.to_be_bytes()[3..8]);
            out.extend_from_slice(&size.to_le_bytes());
        }
        let mut state = [0u32; 2];
        for entry in out[ENTRIES_OFFSET..].chunks_exact(ENTRY_SIZE) {
            hash_index(entry, &mut state);
        }
        out[36..40].copy_from_slice(&state[0].to_le_bytes());
        let mut state = [0u32; 2];
        hash_index(&out[8..24], &mut state);
        out[4..8].copy_from_slice(&state[0].to_le_bytes());
        out
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ow-casc-idx-{}-{name}.idx", std::process::id()));
        fs::write(&path, bytes).expect("write synthetic bucket");
        path
    }

    /// Known answers from the client/real files. The 16-byte header is the
    /// real `0300000159.idx` file header and `0xc52f303d` is the hash the
    /// client stored at file offset 4. The two entry states are the rolling
    /// values after the first and second real entries of that file.
    #[test]
    fn matches_the_client_hash_on_known_vectors() {
        fn bytes(hex: &str) -> Vec<u8> {
            hex.as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect()
        }
        let mut state = [0u32; 2];
        hash_index(&bytes("070003000405091e000000c0ff000000"), &mut state);
        assert_eq!(state[0], 0xc52f_303d);

        let mut state = [0u32; 2];
        hash_index(&bytes("00001d9bbc07919efd0fd56f31a2b4220000"), &mut state);
        assert_eq!(state, [0xef6c_3f29, 0xd114_251e]);
        hash_index(&bytes("00003400342943950309a1666c77513a0000"), &mut state);
        assert_eq!(state, [0x758e_09f9, 0x9c58_4663]);
    }

    /// A zero-length segment is the client's switch case 0: it stores the two
    /// running words and returns before the final mix. `parse_bucket_file`
    /// never hashes an empty slice, but the function's contract includes it;
    /// the values are the client's own output for the same inputs.
    #[test]
    fn a_zero_length_hash_returns_the_running_words_unmixed() {
        let mut state = [0u32; 2];
        hash_index(&[], &mut state);
        assert_eq!(state, [0xdead_beef, 0xdead_beef]);

        let mut state = [0xdead_beef, 0x1234_5678];
        hash_index(&[], &mut state);
        assert_eq!(state, [0xcf8f_d456, 0xbd5b_7dde]);
    }

    /// `DynamicIndexKeyHash` (`0x142513230`) known answers, read from the
    /// client's bytes: the empty early return (`case 0` returns `c`), every
    /// tail length from 1 to 12, the 12-byte block loop, and the real 19-byte
    /// page-key shape. The install's page tables are zero-filled, so there is
    /// no stored page-record hash; these are the client's outputs for the same
    /// inputs. `00..18` is also a real 19-byte span of `0300000159.idx`'s first
    /// entry block (the page tables themselves are all zero). The client reads
    /// `init` through `R8D`, so `key_hash` takes a `u32`; the fixture exercises
    /// the oracle's truncation of a 64-bit init word.
    #[test]
    fn matches_the_client_key_hash_on_known_vectors() {
        let seq = |n: usize| -> Vec<u8> { (0..n).map(|i| i as u8).collect() };
        let cases: &[(Vec<u8>, u32, u32)] = &[
            (Vec::new(), 0, 0xdead_beef),
            (Vec::new(), 1, 0xdead_bef0),
            (Vec::new(), 0xffff_ffff, 0xdead_beee),
            (vec![0u8; 19], 0, 0x9100_686e),
            (vec![0u8; 19], 0xffff_ffff, 0x3428_e3f5),
            (seq(19), 0, 0x74ab_ad21),
            (seq(19), 1, 0x6d33_d311),
            (seq(19), 0xffff_ffff, 0x8036_c457),
            (b"hello, world!".to_vec(), 0x1234_5678, 0xb416_2ac5),
            (seq(24), 0, 0x9c0a_dd53),
            (seq(25), 0, 0x3a88_2244),
            (seq(8), 0, 0xd4cc_b93c),
            (seq(9), 0, 0xeca5_1474),
            (seq(10), 0, 0x610f_c1e3),
            (seq(14), 0, 0x9609_e9d3),
            (seq(15), 0, 0x63a6_e503),
            (seq(16), 0, 0x5e84_eafc),
            (seq(17), 0, 0x8c35_32cb),
            (seq(18), 0, 0xd732_003d),
            (seq(23), 0, 0xa5ff_3c58),
            // The first 19 bytes of the real file's entry block.
            (
                hex_bytes("00001d9bbc07919efd0fd56f31a2b422000000"),
                0,
                0x6de3_52b4,
            ),
            (
                hex_bytes("00003400342943950309a1666c77513a000000"),
                0,
                0x1994_5f92,
            ),
        ];
        for (key, init, expected) in cases {
            assert_eq!(
                key_hash(key, *init),
                *expected,
                "key {key:02x?} init {init:#x}"
            );
        }
    }

    /// The Rust slice has no alignment path; the same 19 bytes at four address
    /// offsets must hash identically. The oracle unit runs the client's three
    /// pointer paths (16-bit aligned, even-not-4, odd) and compares each.
    #[test]
    fn key_hash_does_not_depend_on_slice_alignment() {
        let seed = seq_bytes(19);
        let expected = key_hash(&seed, 0);
        for offset in 0..4 {
            let mut buf = [0u8; 32];
            buf[offset..offset + 19].copy_from_slice(&seed);
            assert_eq!(key_hash(&buf[offset..offset + 19], 0), expected);
        }
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }

    fn seq_bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| i as u8).collect()
    }

    /// The client's block loop runs `floor((len - 1) / 12)`, so a trailing
    /// whole twelve-byte group is added to the running words, not mixed as a
    /// block. The `.idx` files only hash 16- and 18-byte spans, so this shape
    /// has no file vector; these are the client's own outputs for `00..0b`
    /// and `00..17` with the state at zero.
    #[test]
    fn a_whole_twelve_byte_tail_is_added_not_mixed() {
        let twelve: Vec<u8> = (0u8..12).collect();
        let mut state = [0u32; 2];
        hash_index(&twelve, &mut state);
        assert_eq!(state, [0x5e4a_a593, 0x0cb6_e7e9]);

        let twenty_four: Vec<u8> = (0u8..24).collect();
        let mut state = [0u32; 2];
        hash_index(&twenty_four, &mut state);
        assert_eq!(state, [0x9c0a_dd53, 0x9f41_1002]);
    }

    #[test]
    fn reads_entries_and_both_endiannesses() {
        let key = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        let archive = 5u64;
        let offset = 0x1234_5678u64;
        let packed = (archive << 30) | offset;
        let path = write_temp("entries", &bucket_file(7, 3, &[(key, packed, 0x1122_3344)]));
        let entries = parse_bucket_file(&path, Some(3)).expect("parses");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, key);
        assert_eq!(
            entries[0].1,
            Location {
                archive: 5,
                offset: 0x1234_5678,
                size: 0x1122_3344,
            }
        );
        fs::remove_file(&path).ok();
    }

    /// The client accepts version >= 7; it only hardcodes the 4/5/9 widths.
    #[test]
    fn accepts_a_newer_version_with_the_same_widths() {
        let key = [7u8; 9];
        let path = write_temp("v8", &bucket_file(8, 0, &[(key, 1 << 30, 7)]));
        let entries = parse_bucket_file(&path, Some(0)).expect("v8 parses");
        assert_eq!(entries[0].1.archive, 1);
        assert_eq!(entries[0].1.offset, 0);
        fs::remove_file(&path).ok();
    }

    /// The entry block is positioned by the section header's byte count, not
    /// by a fixed 0x28. The client aligns `sectionSize + 0x17` to 16 bytes;
    /// this file declares a 32-byte section and must still read.
    #[test]
    fn finds_the_entry_block_from_the_section_size() {
        let key = [0x42u8; 9];
        let section = 32usize;
        let entry_header = (section + 0x17) & !0xf;
        let mut data = vec![0u8; entry_header + 8 + ENTRY_SIZE];
        data[0..4].copy_from_slice(&(section as u32).to_le_bytes());
        data[8..10].copy_from_slice(&7u16.to_le_bytes());
        data[10] = 2;
        data[12] = 4;
        data[13] = 5;
        data[14] = 9;
        data[entry_header..entry_header + 4].copy_from_slice(&(ENTRY_SIZE as i32).to_le_bytes());
        let at = entry_header + 8;
        data[at..at + 9].copy_from_slice(&key);
        data[at + 9..at + 14].copy_from_slice(&(1u64 << 30).to_be_bytes()[3..8]);
        data[at + 14..at + 18].copy_from_slice(&99u32.to_le_bytes());
        let mut state = [0u32; 2];
        hash_index(&data[at..at + ENTRY_SIZE], &mut state);
        data[entry_header + 4..entry_header + 8].copy_from_slice(&state[0].to_le_bytes());
        let mut state = [0u32; 2];
        hash_index(&data[8..8 + section], &mut state);
        data[4..8].copy_from_slice(&state[0].to_le_bytes());

        let path = write_temp("section", &data);
        let entries = parse_bucket_file(&path, Some(2)).expect("parses");
        assert_eq!(entries[0].0, key);
        assert_eq!(entries[0].1.archive, 1);
        assert_eq!(entries[0].1.size, 99);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_an_older_version_and_a_foreign_bucket() {
        let key = [7u8; 9];
        let path = write_temp("v6", &bucket_file(6, 0, &[(key, 0, 7)]));
        assert!(parse_bucket_file(&path, Some(0)).is_err());
        fs::remove_file(&path).ok();

        let path = write_temp("bucket", &bucket_file(7, 1, &[(key, 0, 7)]));
        assert!(parse_bucket_file(&path, Some(2)).is_err());
        fs::remove_file(&path).ok();
    }
}

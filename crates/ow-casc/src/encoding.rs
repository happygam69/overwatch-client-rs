//! The encoding file: content key to encoding key, and decoded sizes.
//!
//! Layout, verified against the real blob (the ordering below is what makes the
//! root file's content key land on its page boundary):
//!
//! ```text
//! header (22 bytes)
//!   u16 LE signature "EN"
//!   u8  version (1)
//!   u8  ckey size (16)
//!   u8  ekey size (16)
//!   u16 BE ckey->ekey page size, in KiB
//!   u16 BE ekey->espec page size, in KiB
//!   u32 BE ckey->ekey page count
//!   u32 BE ekey->espec page count
//!   u8  unknown
//!   u32 BE espec block size
//! espec block
//! ckey->ekey page index   (page count * 32: first key[16], page hash[16])
//! ckey->ekey page bodies  (page count * page size)
//! ekey->espec page index  (page count * 32)
//! ekey->espec page bodies (page count * page size)
//! ```
//!
//! Every page *index* precedes every page *body*; reading them interleaved
//! misaligns the whole file.
//!
//! A ckey entry is `u16 ekey count`, `u32 BE content size`, `ckey[16]`, then
//! that many `ekey[16]`. An espec entry is `ekey[16]`, `u32 BE espec index`,
//! `u40 BE file size`; an index of `0xFFFFFFFF` terminates a page.

use std::fmt;

pub const HEADER_SIZE: usize = 22;
/// The client's parsed-header struct: 40 bytes, written at fixed offsets.
pub const HEADER_FIELDS_SIZE: usize = 40;
pub const PAGE_INDEX_ENTRY: usize = 32;

/// The client's return code for a header it rejects.
pub const HEADER_REJECTED: u32 = 8;

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
    /// The header parsed exactly as the client parses it, but its key widths
    /// are not 16/16. [`EncodingFile`]'s page walk is written for 16-byte keys
    /// only; this is that reader's limit, not the header parser's. The client
    /// accepts every width 1..=16.
    UnsupportedKeyWidth {
        ckey: u8,
        ekey: u8,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed encoding file: {m}"),
            Error::UnsupportedKeyWidth { ckey, ekey } => write!(
                f,
                "encoding header names {ckey}/{ekey}-byte keys; this page reader is fixed at 16/16"
            ),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// The client's `EncodingHeaderParse` (`0x142498920`), operation for
/// operation.
///
/// The client's output is a 40-byte struct, written at these offsets:
///
/// | offset | field |
/// |---|---|
/// | `0x11` | `u8` ckey width |
/// | `0x12` | `u8` ekey width |
/// | `0x14` | `u32` ckey->ekey page count |
/// | `0x18` | `u32` ckey->ekey page bytes |
/// | `0x1c` | `u32` ekey->espec page count |
/// | `0x20` | `u32` ekey->espec page bytes |
/// | `0x24` | `u32` espec block size |
///
/// Every field is big-endian in the file. The client assembles each integer
/// with the same shift-and-or sequence this function uses and stores it
/// host-order; a page size is shifted left by ten to turn its KiB count into
/// bytes.
///
/// The gates are, in order: a `data` pointer, at least 22 bytes, `'E'`,
/// `'N'`, version 1, and a zero byte 17. Only a header that passes all of
/// them is written. The seven value checks (both key widths, both page sizes,
/// both page counts, the espec block) run **after** the writes, so a header
/// that fails one still leaves every field written and returns
/// [`HEADER_REJECTED`]. This function returns 0 only when the header is
/// accepted.
///
/// The client's null-data guard cannot be expressed through a `&[u8]`, and an
/// empty slice would have to fail the 22-byte gate anyway. Every other gate
/// and every write is the client's.
pub fn write_header_fields(data: &[u8], out: &mut [u8; HEADER_FIELDS_SIZE]) -> u32 {
    if data.len() < HEADER_SIZE
        || data[0] != b'E'
        || data[1] != b'N'
        || data[2] != 1
        || data[0x11] != 0
    {
        return HEADER_REJECTED;
    }

    // Write order is the client's: the two widths, the ckey->ekey page size,
    // the ekey->espec page size, the two page counts, then the espec block.
    out[0x11] = data[3];
    out[0x12] = data[4];

    let ck_page = (u32::from(data[5]) << 8) | u32::from(data[6]);
    let ck_page = ck_page << 10;
    out[0x18..0x1c].copy_from_slice(&ck_page.to_le_bytes());

    let es_page = (u32::from(data[7]) << 8) | u32::from(data[8]);
    let es_page = es_page << 10;
    out[0x20..0x24].copy_from_slice(&es_page.to_le_bytes());

    let ck_count = (u32::from(data[9]) << 8) | u32::from(data[10]);
    let ck_count = (ck_count << 8) | u32::from(data[11]);
    let ck_count = (ck_count << 8) | u32::from(data[12]);
    out[0x14..0x18].copy_from_slice(&ck_count.to_le_bytes());

    let es_count = (u32::from(data[13]) << 8) | u32::from(data[14]);
    let es_count = (es_count << 8) | u32::from(data[15]);
    let es_count = (es_count << 8) | u32::from(data[16]);
    out[0x1c..0x20].copy_from_slice(&es_count.to_le_bytes());

    let espec = (u32::from(data[18]) << 8) | u32::from(data[19]);
    let espec = (espec << 8) | u32::from(data[20]);
    let espec = (espec << 8) | u32::from(data[21]);
    out[0x24..0x28].copy_from_slice(&espec.to_le_bytes());

    // The client's value checks, after the writes. The ckey gate is
    // `zero or > 16`; the ekey gate is `(u8)(ekey - 1) > 15`, which is the
    // same 1..=16 range with a wrapping subtract.
    let ck_width = data[3];
    let ek_width = data[4];
    if ck_width == 0 {
        return HEADER_REJECTED;
    }
    if ck_width > 0x10 {
        return HEADER_REJECTED;
    }
    if ek_width.wrapping_sub(1) > 0x0f {
        return HEADER_REJECTED;
    }
    if ck_page == 0 {
        return HEADER_REJECTED;
    }
    if es_page == 0 {
        return HEADER_REJECTED;
    }
    if ck_count == 0 {
        return HEADER_REJECTED;
    }
    if es_count == 0 {
        return HEADER_REJECTED;
    }
    if espec == 0 {
        return HEADER_REJECTED;
    }
    0
}

/// A header the client accepted, in the client's own 40-byte layout.
///
/// [`EncodingHeader::from_bytes`] calls [`write_header_fields`] and keeps the
/// result only when it returns 0, so the accept/reject rule is the shipped
/// function's, not a second copy. The buffer starts zeroed; bytes the client
/// does not write stay zero here (the client leaves whatever the caller put
/// there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingHeader {
    fields: [u8; HEADER_FIELDS_SIZE],
}

impl EncodingHeader {
    /// Parse `data` with the client's `EncodingHeaderParse` contract.
    pub fn from_bytes(data: &[u8]) -> Option<EncodingHeader> {
        let mut fields = [0u8; HEADER_FIELDS_SIZE];
        if write_header_fields(data, &mut fields) != 0 {
            return None;
        }
        Some(EncodingHeader { fields })
    }

    /// The raw 40-byte struct, exactly as the client wrote it.
    pub fn fields(&self) -> &[u8; HEADER_FIELDS_SIZE] {
        &self.fields
    }

    pub fn ckey_width(&self) -> u8 {
        self.fields[0x11]
    }

    pub fn ekey_width(&self) -> u8 {
        self.fields[0x12]
    }

    pub fn ckey_to_ekey_page_count(&self) -> u32 {
        self.u32_at(0x14)
    }

    pub fn ckey_to_ekey_page_bytes(&self) -> u32 {
        self.u32_at(0x18)
    }

    pub fn ekey_to_espec_page_count(&self) -> u32 {
        self.u32_at(0x1c)
    }

    pub fn ekey_to_espec_page_bytes(&self) -> u32 {
        self.u32_at(0x20)
    }

    pub fn espec_block_size(&self) -> u32 {
        self.u32_at(0x24)
    }

    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.fields[at..at + 4].try_into().expect("4-byte field"))
    }
}

pub struct EncodingFile<'a> {
    data: &'a [u8],
    pub ckey_size: usize,
    pub ekey_size: usize,
    ck_page_size: usize,
    es_page_size: usize,
    n_ck_pages: usize,
    n_es_pages: usize,
    /// Start of the ckey->ekey page index.
    ck_index: usize,
    /// Start of the ckey->ekey page bodies.
    ck_bodies: usize,
    /// Start of the ekey->espec page index.
    es_index: usize,
    /// Start of the ekey->espec page bodies.
    es_bodies: usize,
}

impl<'a> EncodingFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<EncodingFile<'a>> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Malformed("shorter than its header"));
        }
        // One parser for the shipped reader and the oracle unit: the client's
        // own accept/reject rule. `from_bytes` discards the buffer on a
        // reject, so a bad header still resolves nothing.
        let header = EncodingHeader::from_bytes(data)
            .ok_or(Error::Malformed("rejected by EncodingHeaderParse"))?;

        let ckey_size = header.ckey_width() as usize;
        let ekey_size = header.ekey_width() as usize;
        // The client's header parser accepts every key width 1..=16 and
        // stores it. This page reader's binary search and record walk are
        // written for 16-byte keys, so a header with another width is refused
        // explicitly rather than read at the wrong stride. This is the page
        // reader's limit, not the client header parser's.
        if ckey_size != 16 || ekey_size != 16 {
            return Err(Error::UnsupportedKeyWidth {
                ckey: header.ckey_width(),
                ekey: header.ekey_width(),
            });
        }

        let ck_page_size = header.ckey_to_ekey_page_bytes() as usize;
        let es_page_size = header.ekey_to_espec_page_bytes() as usize;
        let n_ck_pages = header.ckey_to_ekey_page_count() as usize;
        let n_es_pages = header.ekey_to_espec_page_count() as usize;
        let espec_size = header.espec_block_size() as usize;

        let ck_index = HEADER_SIZE + espec_size;
        let ck_bodies = ck_index + n_ck_pages * PAGE_INDEX_ENTRY;
        let es_index = ck_bodies + n_ck_pages * ck_page_size;
        let es_bodies = es_index + n_es_pages * PAGE_INDEX_ENTRY;
        let end = es_bodies + n_es_pages * es_page_size;
        if data.len() < end {
            return Err(Error::Malformed("sections run past the end of the file"));
        }

        Ok(EncodingFile {
            data,
            ckey_size,
            ekey_size,
            ck_page_size,
            es_page_size,
            n_ck_pages,
            n_es_pages,
            ck_index,
            ck_bodies,
            es_index,
            es_bodies,
        })
    }

    fn page_first_key(&self, index_start: usize, page: usize) -> Option<[u8; 16]> {
        let at = index_start + page * PAGE_INDEX_ENTRY;
        let slice = self.data.get(at..at + 16)?;
        let mut key = [0u8; 16];
        key.copy_from_slice(slice);
        Some(key)
    }

    /// Binary search the page index for the page that would contain `key`.
    fn page_for(&self, index_start: usize, pages: usize, key: &[u8; 16]) -> Option<usize> {
        let mut lo = 0usize;
        let mut hi = pages;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let first = self.page_first_key(index_start, mid)?;
            if &first <= key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo.checked_sub(1)
    }

    /// Encoding keys that store this content key.
    pub fn ekeys_for_ckey(&self, ckey: &[u8; 16]) -> Vec<[u8; 16]> {
        let Some(page) = self.page_for(self.ck_index, self.n_ck_pages, ckey) else {
            return Vec::new();
        };
        let start = self.ck_bodies + page * self.ck_page_size;
        let body = &self.data[start..start + self.ck_page_size];

        let mut p = 0usize;
        while p + 22 <= body.len() {
            let count = u16::from_le_bytes([body[p], body[p + 1]]) as usize;
            if count == 0 {
                break;
            }
            let entry_ckey = &body[p + 6..p + 22];
            p += 22;
            let ekeys_end = p + count * 16;
            if ekeys_end > body.len() {
                break;
            }
            if entry_ckey == ckey {
                return body[p..ekeys_end]
                    .chunks(16)
                    .map(|c| {
                        let mut k = [0u8; 16];
                        k.copy_from_slice(c);
                        k
                    })
                    .collect();
            }
            p = ekeys_end;
        }
        Vec::new()
    }

    /// Declared decoded size for an encoding key.
    pub fn decoded_size(&self, ekey: &[u8; 16]) -> Option<u64> {
        let page = self.page_for(self.es_index, self.n_es_pages, ekey)?;
        let start = self.es_bodies + page * self.es_page_size;
        let body = &self.data[start..start + self.es_page_size];

        let mut p = 0usize;
        while p + 25 <= body.len() {
            let entry_key = &body[p..p + 16];
            let espec =
                u32::from_be_bytes([body[p + 16], body[p + 17], body[p + 18], body[p + 19]]);
            let size = ((body[p + 20] as u64) << 32)
                | ((body[p + 21] as u64) << 24)
                | ((body[p + 22] as u64) << 16)
                | ((body[p + 23] as u64) << 8)
                | body[p + 24] as u64;
            p += 25;
            if espec == 0xFFFF_FFFF {
                break;
            }
            if entry_key == ekey {
                return Some(size);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Vec<u8> {
        let mut h = vec![0u8; HEADER_SIZE];
        h[0..2].copy_from_slice(b"EN");
        h[2] = 1;
        h[3] = 16;
        h[4] = 16;
        h[5..7].copy_from_slice(&4u16.to_be_bytes());
        h[7..9].copy_from_slice(&4u16.to_be_bytes());
        h[9..13].copy_from_slice(&1u32.to_be_bytes());
        h[13..17].copy_from_slice(&1u32.to_be_bytes());
        h[18..22].copy_from_slice(&1u32.to_be_bytes());
        h
    }

    /// A minimal well-formed file: one 4 KiB page on each side, a one-byte
    /// espec block, and zero-count page bodies (a zero count ends a page).
    fn minimal_file() -> Vec<u8> {
        let h = header();
        let ck_index = HEADER_SIZE + 1;
        let ck_bodies = ck_index + PAGE_INDEX_ENTRY;
        let es_index = ck_bodies + 4096;
        let es_bodies = es_index + PAGE_INDEX_ENTRY;
        let mut out = vec![0u8; es_bodies + 4096];
        out[..HEADER_SIZE].copy_from_slice(&h);
        out
    }

    /// The install's real encoding header (`111,058,223` decoded bytes):
    /// `'EN'`, version 1, 16/16 keys, 4 KiB pages, 16025 + 10519 pages,
    /// byte 17 zero, and a 1,467,099-byte espec block.
    const REAL_HEADER: [u8; HEADER_SIZE] = [
        0x45, 0x4e, 0x01, 0x10, 0x10, 0x00, 0x04, 0x00, 0x04, 0x00, 0x00, 0x3e, 0x99, 0x00, 0x00,
        0x29, 0x17, 0x00, 0x00, 0x16, 0xa6, 0xdb,
    ];

    /// The fields the client's function writes for [`REAL_HEADER`], with the
    /// buffer's untouched bytes left at `pattern`.
    fn expected_real_fields(pattern: u8) -> [u8; HEADER_FIELDS_SIZE] {
        let mut want = [pattern; HEADER_FIELDS_SIZE];
        want[0x11] = 16;
        want[0x12] = 16;
        want[0x14..0x18].copy_from_slice(&16025u32.to_le_bytes());
        want[0x18..0x1c].copy_from_slice(&4096u32.to_le_bytes());
        want[0x1c..0x20].copy_from_slice(&10519u32.to_le_bytes());
        want[0x20..0x24].copy_from_slice(&4096u32.to_le_bytes());
        want[0x24..0x28].copy_from_slice(&0x0016_a6dbu32.to_le_bytes());
        want
    }

    #[test]
    fn header_parse_writes_the_installs_real_header() {
        let mut out = [0xA5u8; HEADER_FIELDS_SIZE];
        assert_eq!(write_header_fields(&REAL_HEADER, &mut out), 0);
        assert_eq!(out, expected_real_fields(0xA5));
    }

    #[test]
    fn header_parse_rejects_the_gate_cases_without_writing() {
        let mut out = [0xA5u8; HEADER_FIELDS_SIZE];
        assert_eq!(write_header_fields(&[], &mut out), HEADER_REJECTED);
        assert_eq!(out, [0xA5u8; HEADER_FIELDS_SIZE]);

        for len in 1..HEADER_SIZE {
            let mut out = [0xA5u8; HEADER_FIELDS_SIZE];
            assert_eq!(
                write_header_fields(&REAL_HEADER[..len], &mut out),
                HEADER_REJECTED,
                "length {len} must be rejected"
            );
            assert_eq!(out, [0xA5u8; HEADER_FIELDS_SIZE]);
        }

        for (at, value) in [(0usize, b'D'), (1, b'M'), (2, 2), (0x11, 1)] {
            let mut bad = REAL_HEADER;
            bad[at] = value;
            let mut out = [0xA5u8; HEADER_FIELDS_SIZE];
            assert_eq!(
                write_header_fields(&bad, &mut out),
                HEADER_REJECTED,
                "byte {at:#x} = {value:#x} must be rejected"
            );
            assert_eq!(out, [0xA5u8; HEADER_FIELDS_SIZE]);
        }
    }

    #[test]
    fn header_parse_writes_all_fields_before_a_value_reject() {
        // A zero ckey width fails the client's first value check, after every
        // field has been written. The fields must therefore still change.
        let mut bad = REAL_HEADER;
        bad[3] = 0;
        let mut out = [0xA5u8; HEADER_FIELDS_SIZE];
        assert_eq!(write_header_fields(&bad, &mut out), HEADER_REJECTED);
        let mut want = expected_real_fields(0xA5);
        want[0x11] = 0;
        assert_eq!(out, want);

        // The same for a zero espec block, the last value check.
        let mut bad = REAL_HEADER;
        bad[18..22].copy_from_slice(&0u32.to_be_bytes());
        let mut out = [0xA5u8; HEADER_FIELDS_SIZE];
        assert_eq!(write_header_fields(&bad, &mut out), HEADER_REJECTED);
        assert_eq!(out[0x11], 16);
        assert_eq!(out[0x12], 16);
        assert_eq!(out[0x14..0x18], 16025u32.to_le_bytes());
        assert_eq!(out[0x18..0x1c], 4096u32.to_le_bytes());
        assert_eq!(out[0x1c..0x20], 10519u32.to_le_bytes());
        assert_eq!(out[0x20..0x24], 4096u32.to_le_bytes());
        assert_eq!(out[0x24..0x28], [0; 4]);
        assert_eq!(out[0..0x11], [0xA5u8; 0x11]);
        assert_eq!(out[0x13], 0xA5);
    }

    #[test]
    fn header_wrapper_uses_the_shipped_parse() {
        let header = EncodingHeader::from_bytes(&REAL_HEADER).expect("client-accepted header");
        assert_eq!(header.ckey_width(), 16);
        assert_eq!(header.ekey_width(), 16);
        assert_eq!(header.ckey_to_ekey_page_count(), 16025);
        assert_eq!(header.ckey_to_ekey_page_bytes(), 4096);
        assert_eq!(header.ekey_to_espec_page_count(), 10519);
        assert_eq!(header.ekey_to_espec_page_bytes(), 4096);
        assert_eq!(header.espec_block_size(), 0x0016_a6db);
        assert_eq!(header.fields(), &expected_real_fields(0));

        assert!(EncodingHeader::from_bytes(&REAL_HEADER[..HEADER_SIZE - 1]).is_none());
        let mut bad = REAL_HEADER;
        bad[4] = 0;
        assert!(EncodingHeader::from_bytes(&bad).is_none());
    }

    #[test]
    fn page_reader_refuses_widths_it_cannot_walk() {
        // The client's header parser accepts 1..=16. This reader is fixed at
        // 16-byte keys, so the wrapper accepts these bytes while the page
        // reader must say why it cannot use them.
        let mut file = minimal_file();
        file[3] = 1;
        file[4] = 1;
        assert!(EncodingHeader::from_bytes(&file).is_some());
        assert!(matches!(
            EncodingFile::parse(&file),
            Err(Error::UnsupportedKeyWidth { ckey: 1, ekey: 1 })
        ));

        let mut file = minimal_file();
        file[4] = 15;
        assert!(matches!(
            EncodingFile::parse(&file),
            Err(Error::UnsupportedKeyWidth { ckey: 16, ekey: 15 })
        ));
    }

    #[test]
    fn rejects_the_header_shapes_the_client_rejects() {
        assert!(EncodingFile::parse(&minimal_file()).is_ok());

        let mut bad = minimal_file();
        bad[17] = 1;
        assert!(EncodingFile::parse(&bad).is_err());

        let mut bad = minimal_file();
        bad[9..13].copy_from_slice(&0u32.to_be_bytes());
        assert!(EncodingFile::parse(&bad).is_err());

        let mut bad = minimal_file();
        bad[18..22].copy_from_slice(&0u32.to_be_bytes());
        assert!(EncodingFile::parse(&bad).is_err());

        let mut bad = minimal_file();
        bad[2] = 2;
        assert!(EncodingFile::parse(&bad).is_err());
    }
}

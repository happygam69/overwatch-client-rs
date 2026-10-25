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
pub const PAGE_INDEX_ENTRY: usize = 32;

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed encoding file: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

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
        let signature = u16::from_le_bytes([data[0], data[1]]);
        if signature != 0x4E45 {
            return Err(Error::Malformed("bad signature (expected 'EN')"));
        }
        let version = data[2];
        let ckey_size = data[3] as usize;
        let ekey_size = data[4] as usize;
        if version != 1 || ckey_size != 16 || ekey_size != 16 {
            return Err(Error::Malformed("unsupported version or key size"));
        }

        let ck_page_kb = u16::from_be_bytes([data[5], data[6]]) as usize;
        let es_page_kb = u16::from_be_bytes([data[7], data[8]]) as usize;
        let n_ck_pages = u32::from_be_bytes([data[9], data[10], data[11], data[12]]) as usize;
        let n_es_pages = u32::from_be_bytes([data[13], data[14], data[15], data[16]]) as usize;
        let espec_size = u32::from_be_bytes([data[18], data[19], data[20], data[21]]) as usize;

        // The client's header parser (`FUN_142498920`) requires byte 17 to be
        // zero and every one of the two page sizes, two page counts and the
        // espec block to be non-zero. A file that fails those is rejected
        // before any page is touched; matching that keeps a corrupt header
        // from turning into an empty table that silently resolves nothing.
        if data[17] != 0 {
            return Err(Error::Malformed("header byte 17 is not zero"));
        }
        if ck_page_kb == 0 || es_page_kb == 0 {
            return Err(Error::Malformed("zero page size"));
        }
        if n_ck_pages == 0 || n_es_pages == 0 || espec_size == 0 {
            return Err(Error::Malformed("empty page table or espec block"));
        }

        let ck_page_size = ck_page_kb * 1024;
        let es_page_size = es_page_kb * 1024;

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

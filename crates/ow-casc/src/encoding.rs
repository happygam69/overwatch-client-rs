//! The encoding file: content key to encoding key, and decoded sizes.
//!
//! The section layout is the client's own. `EncodingSectionsRead`
//! (`0x1424d1c60`) lays the file out in five sections, in this order:
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
//! espec block                    offset 0x16, size = header +0x24
//! ckey->ekey page index           (ckey size + 16) * ckey page count
//! ckey->ekey page bodies          ckey page bytes * ckey page count
//! ekey->espec page index          (ekey size + 16) * ekey page count
//! ekey->espec page bodies         ekey page bytes * ekey page count
//! ```
//!
//! Every page *index* precedes every page *body*; reading them interleaved
//! misaligns the whole file. An index entry is `first key[width]` followed by
//! a 16-byte MD5 of that page's body (measured on all 26,544 pages of the
//! installed file). The read-path lookup never compares that hash: it is only
//! the `width + 16` stride, as `EncodingPageIndexSearch` (`0x142498880`)
//! shows. No MD5 call occurs in the two record scans or the resolve.
//!
//! A ckey entry is `u16 ekey count`, `u32 BE decoded size`, `ckey[16]`, then
//! that many `ekey[16]`. The count's **low byte** is the number of ekeys and
//! a zero low byte ends the page's scan; `FUN_142498540` never reads the high
//! byte. An espec entry is `ekey[16]`, `u32 BE espec index`, `u40 BE stored
//! size`; an index of `0xFFFFFFFF` terminates a page. `FUN_142498700` reads
//! only the **low four bytes** of that `u40` into the resolve struct and the
//! ekey is the MD5 of the stored bytes. The espec block is a run of
//! NUL-terminated strings; `FUN_142498a70` (the client's post-read step)
//! rejects a block whose last byte is not zero, and the number of NUL bytes is
//! the number of strings.

use std::fmt;

pub const HEADER_SIZE: usize = 22;
/// The client's parsed-header struct: 40 bytes, written at fixed offsets.
pub const HEADER_FIELDS_SIZE: usize = 40;
/// A page index entry for 16-byte keys: the first key plus its page hash.
pub const PAGE_INDEX_ENTRY: usize = 32;
/// The page hash that follows each page's first key.
pub const PAGE_HASH_SIZE: usize = 16;

/// The client's return code for a header it rejects.
pub const HEADER_REJECTED: u32 = 8;
/// `FUN_142498a70`'s return code for an espec block that does not end in NUL.
pub const ESPEC_NOT_TERMINATED: u32 = 8;

#[derive(Debug)]
pub enum Error {
    /// The file is shorter than the 22-byte header. The client's header read
    /// is clamped and its completion reports status 14.
    HeaderShort,
    /// `EncodingHeaderParse` (`0x142498920`) returned 8.
    HeaderRejected,
    /// The client's range reader (`FUN_142494d10`) rejected the request:
    /// `offset > stream_len || (offset == stream_len && size != 0)`. The
    /// request is marked with state 3; `EncodingSectionsRead`'s drain
    /// (`FUN_142496bd0`) notifies its completion, the result lookup finds no
    /// result, and the reader merges status 3.
    SectionOutOfRange {
        section: SectionName,
        offset: u32,
        size: u32,
        blob_len: u32,
    },
    /// A section request was clamped to the end of the stream; the client's
    /// completion (`FUN_142495470`) turns the short-read flag into status 14.
    SectionShort {
        section: SectionName,
        offset: u32,
        wanted: u32,
        got: u32,
    },
    /// The section's 32-bit arithmetic wrapped in a way the client does not
    /// handle deterministically: a zero size whose clamp reads to the end of
    /// the stream, or an `offset + size` whose 32-bit sum wraps so the clamp
    /// is skipped. A section from a header whose products fit is never this.
    SectionSizeWrap {
        section: SectionName,
        offset: u32,
        size: u32,
        blob_len: u32,
    },
    /// `FUN_142498a70` (`0x142498a70`) returned 8: the espec block's last
    /// byte is not zero.
    EspecNotTerminated,
    /// The decoded stream is larger than the client's `u32` stream length.
    BlobTooLarge,
    /// The header parsed exactly as the client parses it, but its key widths
    /// are not 16/16. The client reads the sections for every width 1..=16;
    /// only the record walk here needs 16. This is the record reader's limit,
    /// not the client's.
    UnsupportedKeyWidth { ckey: u8, ekey: u8 },
    /// A page count in the header, multiplied by its entry stride, wrapped to
    /// zero, so the section that count sized is shorter than the index it has
    /// to hold. The client's header parser never compares the two: it accepts
    /// the header and reads past the end of the index on its first lookup. This
    /// port refuses the layout, like `parse_index_footer` returns 8 where the
    /// client would fault.
    SectionCountMismatch {
        section: SectionName,
        pages: u32,
        stride: u32,
        size: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::HeaderShort => write!(f, "encoding file is shorter than its 22-byte header"),
            Error::HeaderRejected => write!(f, "encoding header was rejected (status 8)"),
            Error::SectionOutOfRange {
                section,
                offset,
                size,
                blob_len,
            } => write!(
                f,
                "{} section at {offset:#x} (+{size}) is past the {blob_len}-byte stream",
                section.as_str()
            ),
            Error::SectionShort {
                section,
                offset,
                wanted,
                got,
            } => write!(
                f,
                "{} section at {offset:#x} wanted {wanted} B but the stream ended after {got}",
                section.as_str()
            ),
            Error::SectionSizeWrap {
                section,
                offset,
                size,
                blob_len,
            } => write!(
                f,
                "{} section's 32-bit size {size} at {offset:#x} does not land inside the {blob_len}-byte stream",
                section.as_str()
            ),
            Error::EspecNotTerminated => {
                write!(f, "espec block does not end in a NUL byte (status 8)")
            }
            Error::BlobTooLarge => {
                write!(f, "encoding stream is larger than the client's u32 length")
            }
            Error::UnsupportedKeyWidth { ckey, ekey } => write!(
                f,
                "encoding header names {ckey}/{ekey}-byte keys; this record reader is fixed at 16/16"
            ),
            Error::SectionCountMismatch {
                section,
                pages,
                stride,
                size,
            } => write!(
                f,
                "{} section is {size} B but {pages} pages of {stride} B need {} B (a 32-bit product wrapped)",
                section.as_str(),
                u64::from(*pages) * u64::from(*stride)
            ),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// The status the client leaf that rejected the input returns.
    ///
    /// | error | status | leaf |
    /// |---|---|---|
    /// | [`Error::HeaderRejected`] | 8 | `EncodingHeaderParse` |
    /// | [`Error::HeaderShort`] | 14 | the header read clamped |
    /// | [`Error::SectionOutOfRange`] | 3 | the reader's result lookup finds no result for the rejected request |
    /// | [`Error::SectionShort`] | 14 | `FUN_142495470` on a short read |
    /// | [`Error::EspecNotTerminated`] | 8 | `FUN_142498a70` |
    /// | the rest | not established | wrapped arithmetic or this reader's width limit |
    ///
    /// A header-parser reject is **not** the encoding open's final status:
    /// `EncodingHeaderRead` (`0x1424d1b40`) replaces the parser's 8 with 14
    /// before the reader sees it. See [`Error::open_status`].
    pub fn status(&self) -> Option<u8> {
        match self {
            Error::HeaderRejected | Error::EspecNotTerminated => Some(8),
            Error::HeaderShort | Error::SectionShort { .. } => Some(14),
            Error::SectionOutOfRange { .. } => Some(3),
            Error::SectionSizeWrap { .. }
            | Error::BlobTooLarge
            | Error::UnsupportedKeyWidth { .. }
            | Error::SectionCountMismatch { .. } => None,
        }
    }

    /// The status the encoding open's state machine (`FUN_1424d16c0`) finishes
    /// with, where that status has been read.
    ///
    /// `EncodingHeaderRead` (`0x1424d1b40`) remaps a header parser reject to
    /// 14, so [`Error::HeaderRejected`] is 8 at the parser and 14 at the open.
    /// A short section read is 14 through the completion's short flag, and a
    /// range rejection is 3 through the no-result lookup; an espec block
    /// without a trailing NUL is 8 through `FUN_142498a70`. The wrapped
    /// arithmetic cases are not claimed.
    pub fn open_status(&self) -> Option<u8> {
        match self {
            Error::HeaderRejected | Error::HeaderShort | Error::SectionShort { .. } => Some(14),
            Error::EspecNotTerminated => Some(8),
            Error::SectionOutOfRange { .. } => Some(3),
            Error::SectionSizeWrap { .. }
            | Error::BlobTooLarge
            | Error::UnsupportedKeyWidth { .. }
            | Error::SectionCountMismatch { .. } => None,
        }
    }
}

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

/// One section of the encoding stream: an absolute offset and a byte count,
/// both in the client's `u32` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub offset: u32,
    pub size: u32,
}

impl Section {
    /// Exclusive end in 64-bit arithmetic. The client's own sums are 32-bit;
    /// this is only for reporting and the stream-length check.
    pub fn end(&self) -> u64 {
        u64::from(self.offset) + u64::from(self.size)
    }
}

/// The five sections `EncodingSectionsRead` (`0x1424d1c60`) places, in the
/// client's read order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionLayout {
    pub espec: Section,
    pub ckey_page_index: Section,
    pub ckey_page_bodies: Section,
    pub ekey_page_index: Section,
    pub ekey_page_bodies: Section,
}

impl SectionLayout {
    /// The arithmetic of `EncodingSectionsRead` (`0x1424d1c60`), operation for
    /// operation.
    ///
    /// The client computes every offset and size in a 32-bit register
    /// (`IMUL`/`ADD` on `EAX`/`EDI`) and hands the bit pattern to its range
    /// reader as a `u32`. A product or sum that overflows therefore wraps here
    /// exactly as it does there. The first section starts at `0x16`, the
    /// header's 22 bytes:
    ///
    /// ```text
    /// offset = espec_size + 0x16
    /// ck index   = (ckey_width + 0x10) * ck_pages     at offset
    /// ck bodies  = ck_page_bytes * ck_pages
    /// ek index   = (ekey_width + 0x10) * ek_pages
    /// ek bodies  = ek_page_bytes * ek_pages
    /// ```
    ///
    /// Each offset is the previous offset plus the previous size, added in
    /// the same order the client adds them.
    pub fn of(fields: &[u8; HEADER_FIELDS_SIZE]) -> SectionLayout {
        let ckey_width = i32::from(fields[0x11]);
        let ekey_width = i32::from(fields[0x12]);
        let ck_pages = i32_at(fields, 0x14);
        let ck_page_bytes = i32_at(fields, 0x18);
        let ek_pages = i32_at(fields, 0x1c);
        let ek_page_bytes = i32_at(fields, 0x20);
        let espec_size = i32_at(fields, 0x24);

        let espec = Section {
            offset: 0x16,
            size: espec_size as u32,
        };
        let mut offset = espec_size.wrapping_add(0x16);

        let ck_index_size = ckey_width.wrapping_add(0x10).wrapping_mul(ck_pages);
        let ckey_page_index = Section {
            offset: offset as u32,
            size: ck_index_size as u32,
        };
        offset = offset.wrapping_add(ck_index_size);

        let ck_bodies_size = ck_page_bytes.wrapping_mul(ck_pages);
        let ckey_page_bodies = Section {
            offset: offset as u32,
            size: ck_bodies_size as u32,
        };
        offset = offset.wrapping_add(ck_bodies_size);

        let ek_index_size = ekey_width.wrapping_add(0x10).wrapping_mul(ek_pages);
        let ekey_page_index = Section {
            offset: offset as u32,
            size: ek_index_size as u32,
        };
        offset = offset.wrapping_add(ek_index_size);

        let ek_bodies_size = ek_page_bytes.wrapping_mul(ek_pages);
        let ekey_page_bodies = Section {
            offset: offset as u32,
            size: ek_bodies_size as u32,
        };

        SectionLayout {
            espec,
            ckey_page_index,
            ckey_page_bodies,
            ekey_page_index,
            ekey_page_bodies,
        }
    }

    /// The five sections with their names, in the client's read order.
    pub fn named(&self) -> [(SectionName, Section); 5] {
        [
            (SectionName::Espec, self.espec),
            (SectionName::CkeyPageIndex, self.ckey_page_index),
            (SectionName::CkeyPageBodies, self.ckey_page_bodies),
            (SectionName::EkeyPageIndex, self.ekey_page_index),
            (SectionName::EkeyPageBodies, self.ekey_page_bodies),
        ]
    }
}

/// The five sections' names, for errors and tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionName {
    Espec,
    CkeyPageIndex,
    CkeyPageBodies,
    EkeyPageIndex,
    EkeyPageBodies,
}

impl SectionName {
    pub fn as_str(self) -> &'static str {
        match self {
            SectionName::Espec => "espec",
            SectionName::CkeyPageIndex => "ckey->ekey page index",
            SectionName::CkeyPageBodies => "ckey->ekey page bodies",
            SectionName::EkeyPageIndex => "ekey->espec page index",
            SectionName::EkeyPageBodies => "ekey->espec page bodies",
        }
    }
}

/// The five sections of a decoded stream, sliced.
#[derive(Clone, Copy, Debug)]
pub struct SectionSlices<'a> {
    pub espec: &'a [u8],
    pub ckey_page_index: &'a [u8],
    pub ckey_page_bodies: &'a [u8],
    pub ekey_page_index: &'a [u8],
    pub ekey_page_bodies: &'a [u8],
}

/// Read the five sections from a decoded encoding stream with the client's
/// range rule.
///
/// `FUN_142494d10` is the shared range reader `EncodingSectionsRead` submits
/// each section to. It rejects a request when `offset > stream_len`, or when
/// `offset == stream_len` and `size != 0`. Otherwise it clamps a request that
/// would cross the end to `stream_len - offset` and flags it short; the
/// completion (`FUN_142495470`) reports a short read as status 14.
///
/// The client builds all five requests from the section layout and waits for
/// all of them; a failure of one fails the encoding open. This function is the
/// synchronous equivalent: it returns the five slices, or the first failing
/// section in the client's read order.
pub fn read_sections<'a>(data: &'a [u8], layout: &SectionLayout) -> Result<SectionSlices<'a>> {
    let blob_len = u32::try_from(data.len()).map_err(|_| Error::BlobTooLarge)?;
    Ok(SectionSlices {
        espec: read_section(data, blob_len, SectionName::Espec, layout.espec)?,
        ckey_page_index: read_section(
            data,
            blob_len,
            SectionName::CkeyPageIndex,
            layout.ckey_page_index,
        )?,
        ckey_page_bodies: read_section(
            data,
            blob_len,
            SectionName::CkeyPageBodies,
            layout.ckey_page_bodies,
        )?,
        ekey_page_index: read_section(
            data,
            blob_len,
            SectionName::EkeyPageIndex,
            layout.ekey_page_index,
        )?,
        ekey_page_bodies: read_section(
            data,
            blob_len,
            SectionName::EkeyPageBodies,
            layout.ekey_page_bodies,
        )?,
    })
}

fn read_section(data: &[u8], blob_len: u32, name: SectionName, section: Section) -> Result<&[u8]> {
    // `FUN_142494d10`: `offset > len || (offset == len && size != 0)`.
    if section.offset > blob_len || (section.offset == blob_len && section.size != 0) {
        return Err(Error::SectionOutOfRange {
            section: name,
            offset: section.offset,
            size: section.size,
            blob_len,
        });
    }

    if section.size == 0 {
        // The client's clamp turns a zero size into `len - offset`, then reads
        // that many bytes into a zero-byte allocation. At the very end of the
        // stream that is an empty read; anywhere else it is an overrun with no
        // defined result.
        if section.offset == blob_len {
            return Ok(&data[blob_len as usize..]);
        }
        return Err(Error::SectionSizeWrap {
            section: name,
            offset: section.offset,
            size: section.size,
            blob_len,
        });
    }

    // The client compares the *32-bit* sum `offset + size` with the length.
    // When that wraps, the clamp is skipped and the read runs past the end.
    let sum = section.offset.wrapping_add(section.size);
    let got = if sum > blob_len {
        blob_len - section.offset
    } else {
        section.size
    };
    if got < section.size {
        return Err(Error::SectionShort {
            section: name,
            offset: section.offset,
            wanted: section.size,
            got,
        });
    }
    if section.end() > u64::from(blob_len) {
        return Err(Error::SectionSizeWrap {
            section: name,
            offset: section.offset,
            size: section.size,
            blob_len,
        });
    }
    let start = section.offset as usize;
    Ok(&data[start..start + got as usize])
}

fn i32_at(fields: &[u8; HEADER_FIELDS_SIZE], at: usize) -> i32 {
    i32::from_le_bytes(fields[at..at + 4].try_into().expect("4-byte field"))
}

/// The client's espec string-index build (`FUN_142498a70`, `0x142498a70`),
/// operation for operation on the no-grow path.
///
/// The client takes one pointer, to a struct whose fields are `block size`
/// (`u32` at `+0x24`, here `block.len()`), `block pointer` (`u64` at `+0x28`,
/// here `base`), and a pointer vector at `+0x50` (`buffer` at `+0x58`, `size`
/// at `+0x60`, `capacity` at `+0x64`, here `index`, `size` and `capacity`).
/// In order, it:
///
/// 1. reads the block's last byte - `block[count - 1]` - and returns 8 when it
///    is not zero. The offset is a zero-extended 32-bit `count - 1`
///    (`lea eax, [r8 - 1]` at `0x142498a95`), so for an empty block the read
///    lands at `base + 0xFFFF_FFFF`, not at `base - 1`: in the oracle arena
///    that address is the `0xC3`-filled sentinel page, so an empty block
///    returns 8 there and never reaches the empty return at `0x142498ad5`.
///    A parsed header rejects a zero espec size, so the empty block is
///    never reached in production either; this function returns 8 for it.
/// 2. counts the block's NUL bytes into a 32-bit counter;
/// 3. reserves the vector to that count (the shared vector grow at
///    `0x142388210`, a no-op when `count <= capacity`);
/// 4. returns 0 when the block is empty (unreachable, see 1);
/// 5. walks the block once, and at every NUL byte calls the string-length
///    helper (`0x1426cce30`, which returns the address of the string's NUL
///    terminator, not its length - the client only tests it for zero) on the
///    current string start, returning 8 when that terminator is the string
///    start itself - an empty string, i.e. a NUL at the block start or right
///    after the previous NUL - before appending, and otherwise appends the
///    string start (`base + offset`, wrapping) at `index[size]` and increments
///    `size`. A string start is the block base, or one past the previous NUL.
///    Appends already made stay made when a later empty string returns 8.
///
/// The grow path - a reserve that must allocate, or an append at
/// `size == capacity` - goes through the allocator vtable of the vector at
/// `+0x50` (alloc at `[[vec]] + 0x10`, free at `+0x30`, via `0x142388210`),
/// which is outside this system; like `page_index_search` on a short index,
/// this function panics rather than inventing a buffer. The verify probe
/// rejects a growing case, and every fixture case pins
/// `nul_count <= capacity` with the appends never reaching `capacity`.
///
/// `index` must hold `capacity` slots. The slice reader [`EncodingFile`]
/// calls this with the espec slice's own address as `base`, then stores
/// the string-start offsets (`pointer - base`).
pub fn espec_string_index(
    block: &[u8],
    base: u64,
    _before: u8,
    index: &mut [u64],
    size: &mut u32,
    capacity: u32,
) -> u8 {
    // The client's last-byte read `block[count - 1]` at `0x142498a99`, with
    // the offset a zero-extended 32-bit `count - 1`. For an empty block the
    // read lands at `base + 0xFFFF_FFFF` (the arena sentinel, byte `0xC3`),
    // not at `base - 1`, so the empty block returns 8 before any write.
    if block.is_empty() {
        return ESPEC_NOT_TERMINATED as u8;
    }
    if block.last().copied().unwrap_or(0) != 0 {
        return ESPEC_NOT_TERMINATED as u8;
    }

    // The NUL count, in the client's 32-bit counter.
    let nul_count = block.iter().filter(|&&b| b == 0).count() as u32;
    // The reserve call: a no-op when `nul_count <= capacity`, an allocator
    // vtable grow otherwise (the declared open path).
    if nul_count > capacity {
        panic!("espec_string_index: vector grow at 0x142388210 is outside the system");
    }

    // Walk the block; `start` is the current string's address, exactly as
    // the client advances `r14` to one past each NUL.
    let mut start = base;
    let mut string_start = 0usize;
    for (offset, &byte) in block.iter().enumerate() {
        if byte != 0 {
            continue;
        }
        // The helper at `0x1426cce30` returns the address of the string's NUL
        // terminator (its byte loop and its 0x7efefefe/0x81010101 word test
        // both produce `start + length`), so a zero answer means the string is
        // empty. Here no NUL lies between the string start and this one, so
        // the terminator is `base + offset` and the test is
        // `offset == string_start`; the client returns 8 before the append.
        if offset == string_start {
            return ESPEC_NOT_TERMINATED as u8;
        }
        // The append: at `size == capacity` the client reserves `size + 1`
        // through the allocator vtable (the declared open path).
        if *size == capacity {
            panic!("espec_string_index: vector grow at 0x142388210 is outside the system");
        }
        index[*size as usize] = start;
        *size = size.wrapping_add(1);
        start = base.wrapping_add(offset as u64).wrapping_add(1);
        string_start = offset + 1;
    }
    0
}

#[derive(Debug)]
pub struct EncodingFile<'a> {
    pub ckey_size: usize,
    pub ekey_size: usize,
    layout: SectionLayout,
    ck_page_size: usize,
    es_page_size: usize,
    n_ck_pages: usize,
    n_es_pages: usize,
    espec: &'a [u8],
    /// String-start offsets in `espec`, built by `espec_string_index`
    /// (`FUN_142498a70`): the client stores the absolute pointers, this
    /// slice reader stores them as offsets from the block base.
    espec_index: Vec<u64>,
    ck_index: &'a [u8],
    ck_bodies: &'a [u8],
    es_index: &'a [u8],
    es_bodies: &'a [u8],
}

impl<'a> EncodingFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<EncodingFile<'a>> {
        if data.len() < HEADER_SIZE {
            return Err(Error::HeaderShort);
        }
        // One parser for the shipped reader and the oracle unit: the client's
        // own accept/reject rule. `from_bytes` discards the buffer on a
        // reject, so a bad header still resolves nothing.
        let header = EncodingHeader::from_bytes(data).ok_or(Error::HeaderRejected)?;

        // `EncodingSectionsRead` (`0x1424d1c60`) uses the header struct's own
        // fields, not a second parse. The layout is the client's arithmetic.
        let layout = SectionLayout::of(header.fields());
        let sections = read_sections(data, &layout)?;

        let ckey_size = header.ckey_width() as usize;
        let ekey_size = header.ekey_width() as usize;

        // `FUN_142498a70`, the client's post-read step: the espec block is a
        // run of NUL-terminated strings whose last byte must be zero, and
        // the string-start index is built from its NUL bytes. The header
        // parser rejects a zero espec size, so the block is never empty
        // here and `before` is never read. The index buffer holds the whole
        // block, so the reserve and every append are no-grow. This slice
        // reader stores the client pointers as offsets from the block base.
        let espec_base = sections.espec.as_ptr() as u64;
        let mut espec_index = vec![0u64; sections.espec.len()];
        let mut espec_size = 0u32;
        let espec_capacity = u32::try_from(sections.espec.len()).unwrap_or(u32::MAX);
        if espec_string_index(
            sections.espec,
            espec_base,
            0,
            &mut espec_index,
            &mut espec_size,
            espec_capacity,
        ) != 0
        {
            return Err(Error::EspecNotTerminated);
        }
        for pointer in espec_index.iter_mut() {
            *pointer = pointer.wrapping_sub(espec_base);
        }
        espec_index.truncate(espec_size as usize);

        // The client's header parser accepts every key width 1..=16 and the
        // section reader uses `width + 16` for an index entry. The record walk
        // below is written for 16-byte keys, so a header with another width is
        // refused explicitly rather than read at the wrong stride. This is the
        // record reader's limit, not the client header parser's.
        if ckey_size != 16 || ekey_size != 16 {
            return Err(Error::UnsupportedKeyWidth {
                ckey: header.ckey_width(),
                ekey: header.ekey_width(),
            });
        }

        // A section's size is the page count times its entry stride, in 32-bit
        // wrapping arithmetic. A header whose product wraps leaves a non-zero
        // page count with a section too short to hold the index
        // `page_index_search` walks; the client reads past the end of it then.
        // The section lengths are those products' low 32 bits, so a mismatch
        // means exactly that wrap, or a corrupt stream, and nothing else.
        let ck_pages = header.ckey_to_ekey_page_count();
        let es_pages = header.ekey_to_espec_page_count();
        let ck_stride = u32::from(header.ckey_width()) + PAGE_HASH_SIZE as u32;
        let es_stride = u32::from(header.ekey_width()) + PAGE_HASH_SIZE as u32;
        for (section, pages, stride, slice) in [
            (
                SectionName::CkeyPageIndex,
                ck_pages,
                ck_stride,
                sections.ckey_page_index,
            ),
            (
                SectionName::EkeyPageIndex,
                es_pages,
                es_stride,
                sections.ekey_page_index,
            ),
        ] {
            let size = u32::try_from(slice.len()).unwrap_or(u32::MAX);
            if u64::from(size) != u64::from(pages) * u64::from(stride) {
                return Err(Error::SectionCountMismatch {
                    section,
                    pages,
                    stride,
                    size,
                });
            }
        }

        Ok(EncodingFile {
            ckey_size,
            ekey_size,
            layout,
            ck_page_size: header.ckey_to_ekey_page_bytes() as usize,
            es_page_size: header.ekey_to_espec_page_bytes() as usize,
            n_ck_pages: header.ckey_to_ekey_page_count() as usize,
            n_es_pages: header.ekey_to_espec_page_count() as usize,
            espec: sections.espec,
            espec_index,
            ck_index: sections.ckey_page_index,
            ck_bodies: sections.ckey_page_bodies,
            es_index: sections.ekey_page_index,
            es_bodies: sections.ekey_page_bodies,
        })
    }

    /// The five sections' offsets and sizes, exactly as the client computed
    /// them.
    pub fn layout(&self) -> &SectionLayout {
        &self.layout
    }

    /// The raw espec block.
    pub fn espec(&self) -> &'a [u8] {
        self.espec
    }

    /// Number of NUL-terminated strings in the espec block: the length of
    /// the string index `FUN_142498a70` built, whose NUL count it counted.
    pub fn espec_entry_count(&self) -> usize {
        self.espec_index.len()
    }

    /// The espec string index: the offset of every NUL-terminated string's
    /// start in the espec block, in order. `FUN_142498a70` stores the
    /// absolute pointers (`base + offset`); this slice reader stores the
    /// offsets.
    pub fn espec_string_offsets(&self) -> &[u64] {
        &self.espec_index
    }

    pub fn ckey_page_count(&self) -> usize {
        self.n_ck_pages
    }

    pub fn ekey_page_count(&self) -> usize {
        self.n_es_pages
    }

    pub fn ckey_page_bytes(&self) -> usize {
        self.ck_page_size
    }

    pub fn ekey_page_bytes(&self) -> usize {
        self.es_page_size
    }

    /// The ckey record for `ckey`, exactly as `FUN_142498540` finds it.
    fn ckey_record(&self, ckey: &[u8; 16]) -> Option<&'a [u8]> {
        let at = find_ckey_record(
            self.ck_index,
            self.ck_bodies,
            self.n_ck_pages as u32,
            self.ckey_size as u8,
            self.ekey_size as u8,
            self.ck_page_size as u32,
            ckey,
            16,
        )?;
        let count = *self.ck_bodies.get(at)? as usize;
        let len = 6 + self.ckey_size + count * self.ekey_size;
        self.ck_bodies.get(at..at + len)
    }

    /// The ekey record for `ekey`, exactly as `FUN_142498610` finds it.
    fn ekey_record(&self, ekey: &[u8; 16]) -> Option<&'a [u8]> {
        let at = find_ekey_record(
            self.es_index,
            self.es_bodies,
            self.n_es_pages as u32,
            self.ekey_size as u8,
            self.es_page_size as u32,
            ekey,
            16,
        )?;
        self.es_bodies.get(at..at + self.ekey_size + 9)
    }

    /// Encoding keys that store this content key.
    ///
    /// The record is the client's; the resolved first ekey is what
    /// `FUN_142498700` copies out. The list is the full record payload: the
    /// client's single-key resolve copies only the first ekey (and returns 7
    /// when the info struct's `+8` field selects the multi-ekey path), while a
    /// caller of this reader may try every ekey in the record.
    pub fn ekeys_for_ckey(&self, ckey: &[u8; 16]) -> Vec<[u8; 16]> {
        let Some(record) = self.ckey_record(ckey) else {
            return Vec::new();
        };
        let count = record[0] as usize;
        let at = 6 + self.ckey_size;
        let end = at + count * self.ekey_size;
        let Some(payload) = record.get(at..end) else {
            return Vec::new();
        };
        payload
            .chunks(self.ekey_size)
            .map(|c| {
                let mut k = [0u8; 16];
                k.copy_from_slice(c);
                k
            })
            .collect()
    }

    /// The ekey->espec record the client's `FUN_142498610` finds.
    ///
    /// `None` means the ekey is not in the file (status 6 in the client's
    /// resolve) or the page is not present.
    pub fn ekey_record_bytes(&self, ekey: &[u8; 16]) -> Option<&'a [u8]> {
        self.ekey_record(ekey)
    }

    /// The stored size the client's resolve reads from an ekey record.
    ///
    /// The record tail is a big-endian `u40`; `FUN_142498700` reads its
    /// **low four bytes** (`record + width + 5 ..= width + 8`) into the
    /// resolve struct's `+0x24` dword and never reads the fifth byte at
    /// `+width + 4`. This function returns that 32-bit value, zero-extended.
    /// The install's 1,714,573 records all have a zero fifth byte, so the
    /// 40-bit and 32-bit sums are equal there.
    pub fn stored_size(&self, ekey: &[u8; 16]) -> Option<u64> {
        let record = self.ekey_record(ekey)?;
        let at = self.ekey_size + 5;
        let bytes = record.get(at..at + 4)?;
        Some(u64::from(u32::from_be_bytes(
            bytes.try_into().expect("4-byte size"),
        )))
    }

    /// The decoded size the client's resolve reads from a ckey record.
    ///
    /// `FUN_142498700` copies the record's big-endian `u32` at `+2` into the
    /// resolve struct's `+0x10`, and `FUN_14248ec20` clamps every read range
    /// against that dword. On the install the first sampled record's value
    /// equals the decoded length exactly (524,304 bytes); the ekey record's
    /// tail is the smaller stored length (3,530).
    pub fn decoded_size(&self, ckey: &[u8; 16]) -> Option<u64> {
        let record = self.ckey_record(ckey)?;
        let bytes = record.get(2..6)?;
        Some(u64::from(u32::from_be_bytes(
            bytes.try_into().expect("4-byte size"),
        )))
    }
}

/// The client's page-index binary search (`FUN_142498880`), operation for
/// operation.
///
/// `index` holds `pages` entries of `width + 16` bytes: the first key
/// (`width` bytes) followed by the page's 16-byte MD5. `key` is compared over
/// its first `key_len` bytes. The search returns the page whose first key is
/// the greatest one `<= key`, the exact page when the key equals a first key,
/// or `u32::MAX` when the key is below the first page.
///
/// The client's arithmetic, in order: `mid = ((hi - lo) >> 1) + lo` on 32-bit
/// unsigned values; `memcmp(key, index + mid * (width + 16), key_len)`; an
/// equal result returns `mid`; a positive result sets `lo = mid + 1`; a
/// negative one sets `hi = mid`; the loop runs while `lo < hi`; the miss is
/// `lo - 1` in 32-bit wrapping arithmetic. The midpoint is therefore the
/// lower-middle, and the result is not a lower bound: it is the last page
/// whose first key does not exceed the key.
///
/// The client reads the entry without a bounds check. Every parsed
/// [`EncodingFile`] has an index slice exactly `pages * (width + 16)` bytes,
/// so `mid` is in range there; this function panics on a shorter slice, like
/// the client would read out of bounds.
pub fn page_index_search(index: &[u8], pages: u32, width: u8, key: &[u8], key_len: u8) -> u32 {
    let len = key_len as usize;
    let key = &key[..len];
    let stride = width as usize + PAGE_HASH_SIZE;
    let mut lo = 0u32;
    let mut hi = pages;
    while lo < hi {
        let mid = ((hi - lo) >> 1) + lo;
        let at = mid as usize * stride;
        let first = &index[at..at + len];
        match first.cmp(key) {
            std::cmp::Ordering::Equal => return mid,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Less => lo = mid + 1,
        }
    }
    lo.wrapping_sub(1)
}

/// The client's ckey page lookup (`FUN_142498540`), operation for operation.
///
/// `index` is the ckey page index (`pages` entries of `ckey_width + 16` bytes);
/// `bodies` is the concatenated page bodies, each `page_bytes` long. The search
/// compares `min(key_len, ckey_width)` bytes, and so does the record scan. The
/// page body starts at `page * page_bytes` in the client's 32-bit arithmetic.
///
/// A record is `u16` little-endian ekey count (only the **low byte** is read,
/// and a zero low byte ends the page), a big-endian `u32` decoded size, the
/// `ckey_width`-byte content key, then `count * ekey_width` bytes of encoding
/// keys. The return is the record's byte offset in `bodies`; the reader
/// derives the length from the count and widths.
///
/// The client reads the body and record without bounds checks. This function
/// stops instead of reading past a short `bodies` slice; for a parsed
/// [`EncodingFile`] the section is exactly `pages * page_bytes` bytes, so that
/// safety edge is unreachable there.
#[rustfmt::skip]
#[allow(clippy::too_many_arguments)]
pub fn find_ckey_record(index: &[u8], bodies: &[u8], pages: u32, ckey_width: u8, ekey_width: u8, page_bytes: u32, key: &[u8], key_len: u8) -> Option<usize> {
    let width = ckey_width as usize;
    let len = (key_len as usize).min(width);
    let key = key.get(..len)?;
    let page = page_index_search(index, pages, ckey_width, key, len as u8);
    if page == u32::MAX {
        return None;
    }
    let start = page.wrapping_mul(page_bytes) as usize;
    let body = bodies.get(start..start + page_bytes as usize)?;
    let mut p = 0usize;
    while p < body.len() {
        let count = body[p] as usize;
        if count == 0 {
            break;
        }
        let Some(entry_key) = body.get(p + 6..p + 6 + width) else {
            break;
        };
        let next = p
            .saturating_add(6)
            .saturating_add(width)
            .saturating_add(count.saturating_mul(ekey_width as usize));
        if &entry_key[..len] == key {
            return Some(start + p);
        }
        p = next;
    }
    None
}

/// The client's ekey page lookup (`FUN_142498610`), operation for operation.
///
/// `index` is the ekey page index (`pages` entries of `ekey_width + 16` bytes);
/// `bodies` is the concatenated page bodies, each `page_bytes` long. The page
/// search compares the caller's **full** `key_len`, while the record scan
/// compares `min(key_len, ekey_width)` bytes. The page body starts at
/// `page * page_bytes` in 32-bit arithmetic.
///
/// A record is `ekey[width]`, a big-endian `u32` espec index, then a big-endian
/// `u40` stored size. An espec index of `0xFFFFFFFF` ends the page's scan. The
/// return is the record's byte offset in `bodies`; the record is `width + 9`
/// bytes.
#[rustfmt::skip]
#[allow(clippy::too_many_arguments)]
pub fn find_ekey_record(index: &[u8], bodies: &[u8], pages: u32, ekey_width: u8, page_bytes: u32, key: &[u8], key_len: u8) -> Option<usize> {
    let width = ekey_width as usize;
    let key_len = key_len as usize;
    if key_len > key.len() {
        return None;
    }
    let page = page_index_search(index, pages, ekey_width, key, key_len as u8);
    if page == u32::MAX {
        return None;
    }
    let start = page.wrapping_mul(page_bytes) as usize;
    let body = bodies.get(start..start + page_bytes as usize)?;
    let len = key_len.min(width);
    let key = &key[..len];
    let mut p = 0usize;
    while p < body.len() {
        let Some(tail) = body.get(p + width..p + width + 4) else {
            break;
        };
        if u32::from_be_bytes(tail.try_into().expect("4-byte espec")) == 0xFFFF_FFFF {
            break;
        }
        let Some(entry_key) = body.get(p..p + width) else {
            break;
        };
        let end = p.saturating_add(width).saturating_add(9);
        if &entry_key[..len] == key {
            return Some(start + p);
        }
        p = end;
    }
    None
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
        h[18..22].copy_from_slice(&2u32.to_be_bytes());
        h
    }

    /// A minimal well-formed file: one 4 KiB page on each side, a two-byte
    /// espec block (`a\0`, one non-empty string - an empty string is
    /// rejected by `FUN_142498a70`), and zero-count page bodies (a zero
    /// count ends a page).
    fn minimal_file() -> Vec<u8> {
        let h = header();
        let ck_index = HEADER_SIZE + 2;
        let ck_bodies = ck_index + PAGE_INDEX_ENTRY;
        let es_index = ck_bodies + 4096;
        let es_bodies = es_index + PAGE_INDEX_ENTRY;
        let mut out = vec![0u8; es_bodies + 4096];
        out[..HEADER_SIZE].copy_from_slice(&h);
        out[HEADER_SIZE] = b'a';
        out
    }

    /// The install's real encoding header (`111,058,223` decoded bytes):
    /// `'EN'`, version 1, 16/16 keys, 4 KiB pages, 16025 + 10519 pages,
    /// byte 17 zero, and a 1,484,507-byte espec block.
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

    fn real_fields() -> [u8; HEADER_FIELDS_SIZE] {
        let mut out = [0u8; HEADER_FIELDS_SIZE];
        assert_eq!(write_header_fields(&REAL_HEADER, &mut out), 0);
        out
    }

    fn field_u32(fields: &mut [u8; HEADER_FIELDS_SIZE], at: usize, value: u32) {
        fields[at..at + 4].copy_from_slice(&value.to_le_bytes());
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

    /// The client's five sections for the installed header:
    ///
    /// ```text
    /// espec              22 + 1,484,507
    /// ck index        1,484,529 + 512,800     = 32 * 16,025
    /// ck bodies       1,997,329 + 65,638,400  = 4,096 * 16,025
    /// ek index       67,635,729 + 336,608     = 32 * 10,519
    /// ek bodies      67,972,337 + 43,085,824  = 4,096 * 10,519
    /// end           111,058,161
    /// ```
    ///
    /// The decoded file is 111,058,223 bytes; the last 62 bytes are outside
    /// every section and are never read by the client.
    #[test]
    fn section_layout_matches_the_installs_header() {
        let layout = SectionLayout::of(&real_fields());
        assert_eq!(
            layout,
            SectionLayout {
                espec: Section {
                    offset: 0x16,
                    size: 1_484_507,
                },
                ckey_page_index: Section {
                    offset: 1_484_529,
                    size: 512_800,
                },
                ckey_page_bodies: Section {
                    offset: 1_997_329,
                    size: 65_638_400,
                },
                ekey_page_index: Section {
                    offset: 67_635_729,
                    size: 336_608,
                },
                ekey_page_bodies: Section {
                    offset: 67_972_337,
                    size: 43_085_824,
                },
            }
        );
        assert_eq!(layout.ekey_page_bodies.end(), 111_058_161);
    }

    /// The layout arithmetic is the client's 32-bit wrapping arithmetic. Two
    /// products that do not fit wrap exactly as `IMUL EAX` does, and a
    /// negative espec size becomes a small positive offset after `+ 0x16`.
    #[test]
    fn section_layout_wraps_in_32_bits_like_the_client() {
        let mut fields = real_fields();
        // 32 * 0x0800_0000 = 0x1_0000_0000 -> 0.
        field_u32(&mut fields, 0x14, 0x0800_0000);
        let layout = SectionLayout::of(&fields);
        assert_eq!(layout.ckey_page_index.size, 0);
        assert_eq!(layout.ckey_page_index.offset, 1_484_529);
        assert_eq!(layout.ckey_page_bodies.offset, 1_484_529);

        // The espec block at 0xffff_fff0 (i32 -16) starts at 6.
        let mut fields = real_fields();
        field_u32(&mut fields, 0x24, 0xffff_fff0);
        let layout = SectionLayout::of(&fields);
        assert_eq!(layout.espec.size, 0xffff_fff0);
        assert_eq!(layout.ckey_page_index.offset, 6);

        // The offset addition wraps too: a 0xffff_fff0 espec block puts the
        // ck index at 6, and its own size (32 * 16,025) moves the bodies on.
        let mut fields = real_fields();
        field_u32(&mut fields, 0x14, 0xffff_ffff);
        let layout = SectionLayout::of(&fields);
        assert_eq!(layout.ckey_page_index.size, 32u32.wrapping_mul(0xffff_ffff));
        assert_eq!(
            layout.ckey_page_bodies.offset,
            1_484_529u32.wrapping_add(32u32.wrapping_mul(0xffff_ffff))
        );
    }

    /// A header whose page-count products wrap to zero leaves every section
    /// empty while naming `0x0800_0000` pages. The client accepts that header
    /// and reads past the empty index on its first page lookup; this port
    /// refuses the layout instead.
    #[test]
    fn a_wrapped_page_count_is_refused_instead_of_read_out_of_bounds() {
        // 44 bytes: 22 header, a 22-byte espec block, and every other section
        // at the end of the stream with size 0.
        let mut blob = vec![0u8; 0x2c];
        blob[0] = b'E';
        blob[1] = b'N';
        blob[2] = 1;
        blob[3] = 16;
        blob[4] = 16;
        blob[6] = 1;
        blob[8] = 1;
        blob[9..13].copy_from_slice(&0x0800_0000u32.to_be_bytes());
        blob[13..17].copy_from_slice(&0x0800_0000u32.to_be_bytes());
        blob[18..22].copy_from_slice(&0x16u32.to_be_bytes());
        // The 22-byte espec block must be non-empty strings (`a\0` x 11),
        // or the espec index build rejects it before the count check.
        for i in 0..11 {
            blob[22 + 2 * i] = b'a';
        }

        let mut fields = [0u8; HEADER_FIELDS_SIZE];
        assert_eq!(write_header_fields(&blob, &mut fields), 0);
        let layout = SectionLayout::of(&fields);
        // 32 * 0x0800_0000 wraps to 0, so every section lands on 0x2c.
        assert_eq!(layout.ckey_page_index.offset, 0x2c);
        assert_eq!(layout.ckey_page_index.size, 0);
        assert_eq!(layout.ekey_page_index.size, 0);
        assert_eq!(layout.ckey_page_bodies.offset, 0x2c);
        assert_eq!(layout.ekey_page_bodies.offset, 0x2c);

        match EncodingFile::parse(&blob) {
            Err(Error::SectionCountMismatch {
                section,
                pages,
                stride,
                size,
            }) => {
                assert_eq!(section, SectionName::CkeyPageIndex);
                assert_eq!(pages, 0x0800_0000);
                assert_eq!(stride, 32);
                assert_eq!(size, 0);
            }
            other => panic!("expected a wrapped page count to be refused, got {other:?}"),
        }
    }

    #[test]
    fn read_sections_accepts_a_section_that_ends_at_the_stream_end() {
        let data = [0u8; 5];
        let layout = SectionLayout {
            espec: Section { offset: 2, size: 3 },
            ckey_page_index: Section { offset: 5, size: 0 },
            ckey_page_bodies: Section { offset: 5, size: 0 },
            ekey_page_index: Section { offset: 5, size: 0 },
            ekey_page_bodies: Section { offset: 5, size: 0 },
        };
        let sliced = read_sections(&data, &layout).expect("sections end at the stream end");
        assert_eq!(sliced.espec, &[0, 0, 0]);
        assert_eq!(sliced.ckey_page_index, &[] as &[u8]);

        // A zero size at the exact end is an empty read, not a failure.
        let layout = SectionLayout {
            espec: Section { offset: 5, size: 0 },
            ckey_page_index: Section { offset: 5, size: 0 },
            ckey_page_bodies: Section { offset: 5, size: 0 },
            ekey_page_index: Section { offset: 5, size: 0 },
            ekey_page_bodies: Section { offset: 5, size: 0 },
        };
        let sliced = read_sections(&data, &layout).expect("zero sizes at the end are empty");
        assert_eq!(sliced.espec, &[] as &[u8]);
    }

    #[test]
    fn read_sections_reports_out_of_range_for_every_section() {
        let data = [0u8; 10];
        for (name, _) in (SectionLayout {
            espec: Section { offset: 0, size: 0 },
            ckey_page_index: Section { offset: 0, size: 0 },
            ckey_page_bodies: Section { offset: 0, size: 0 },
            ekey_page_index: Section { offset: 0, size: 0 },
            ekey_page_bodies: Section { offset: 0, size: 0 },
        })
        .named()
        {
            let mut layout = SectionLayout {
                espec: Section {
                    offset: 10,
                    size: 0,
                },
                ckey_page_index: Section {
                    offset: 10,
                    size: 0,
                },
                ckey_page_bodies: Section {
                    offset: 10,
                    size: 0,
                },
                ekey_page_index: Section {
                    offset: 10,
                    size: 0,
                },
                ekey_page_bodies: Section {
                    offset: 10,
                    size: 0,
                },
            };
            let section = Section {
                offset: 11,
                size: 0,
            };
            match name {
                SectionName::Espec => layout.espec = section,
                SectionName::CkeyPageIndex => layout.ckey_page_index = section,
                SectionName::CkeyPageBodies => layout.ckey_page_bodies = section,
                SectionName::EkeyPageIndex => layout.ekey_page_index = section,
                SectionName::EkeyPageBodies => layout.ekey_page_bodies = section,
            }
            let err = read_sections(&data, &layout).expect_err("past the end");
            match err {
                Error::SectionOutOfRange {
                    section: got,
                    offset,
                    size,
                    blob_len,
                } => {
                    assert_eq!(got, name);
                    assert_eq!((offset, size, blob_len), (11, 0, 10));
                    assert_eq!(
                        Error::SectionOutOfRange {
                            section: got,
                            offset,
                            size,
                            blob_len,
                        }
                        .status(),
                        Some(3)
                    );
                }
                other => panic!("{name:?}: expected out-of-range, got {other:?}"),
            }
        }

        // `offset == len` with a non-zero size is the client's second gate.
        let layout = SectionLayout {
            espec: Section {
                offset: 10,
                size: 1,
            },
            ckey_page_index: Section { offset: 0, size: 0 },
            ckey_page_bodies: Section { offset: 0, size: 0 },
            ekey_page_index: Section { offset: 0, size: 0 },
            ekey_page_bodies: Section { offset: 0, size: 0 },
        };
        assert!(matches!(
            read_sections(&data, &layout),
            Err(Error::SectionOutOfRange {
                section: SectionName::Espec,
                offset: 10,
                size: 1,
                blob_len: 10,
            })
        ));
    }

    #[test]
    fn read_sections_reports_short_reads_for_every_section() {
        let data = [0u8; 10];
        for (name, _) in (SectionLayout {
            espec: Section { offset: 0, size: 0 },
            ckey_page_index: Section { offset: 0, size: 0 },
            ckey_page_bodies: Section { offset: 0, size: 0 },
            ekey_page_index: Section { offset: 0, size: 0 },
            ekey_page_bodies: Section { offset: 0, size: 0 },
        })
        .named()
        {
            let mut layout = SectionLayout {
                espec: Section {
                    offset: 10,
                    size: 0,
                },
                ckey_page_index: Section {
                    offset: 10,
                    size: 0,
                },
                ckey_page_bodies: Section {
                    offset: 10,
                    size: 0,
                },
                ekey_page_index: Section {
                    offset: 10,
                    size: 0,
                },
                ekey_page_bodies: Section {
                    offset: 10,
                    size: 0,
                },
            };
            // Start 4 bytes in, ask for 7: only 6 remain.
            let section = Section { offset: 4, size: 7 };
            match name {
                SectionName::Espec => layout.espec = section,
                SectionName::CkeyPageIndex => layout.ckey_page_index = section,
                SectionName::CkeyPageBodies => layout.ckey_page_bodies = section,
                SectionName::EkeyPageIndex => layout.ekey_page_index = section,
                SectionName::EkeyPageBodies => layout.ekey_page_bodies = section,
            }
            let err = read_sections(&data, &layout).expect_err("short");
            match err {
                Error::SectionShort {
                    section: got,
                    offset,
                    wanted,
                    got: have,
                } => {
                    assert_eq!(got, name);
                    assert_eq!((offset, wanted, have), (4, 7, 6));
                    assert_eq!(
                        Error::SectionShort {
                            section: got,
                            offset,
                            wanted,
                            got: have,
                        }
                        .status(),
                        Some(14)
                    );
                }
                other => panic!("{name:?}: expected short, got {other:?}"),
            }
        }
    }

    #[test]
    fn read_sections_refuses_a_zero_size_that_would_read_to_the_end() {
        let data = [0u8; 10];
        let layout = SectionLayout {
            espec: Section { offset: 3, size: 0 },
            ckey_page_index: Section { offset: 0, size: 0 },
            ckey_page_bodies: Section { offset: 0, size: 0 },
            ekey_page_index: Section { offset: 0, size: 0 },
            ekey_page_bodies: Section { offset: 0, size: 0 },
        };
        assert!(matches!(
            read_sections(&data, &layout),
            Err(Error::SectionSizeWrap {
                section: SectionName::Espec,
                ..
            })
        ));
    }

    #[test]
    fn read_sections_refuses_a_wrapped_offset_size_sum() {
        let data = [0u8; 10];
        // offset + size wraps to 1, which is <= len, so the client skips the
        // clamp and asks for 0xffff_fff9 bytes at offset 8.
        let layout = SectionLayout {
            espec: Section {
                offset: 8,
                size: 0xffff_fff9,
            },
            ckey_page_index: Section { offset: 0, size: 0 },
            ckey_page_bodies: Section { offset: 0, size: 0 },
            ekey_page_index: Section { offset: 0, size: 0 },
            ekey_page_bodies: Section { offset: 0, size: 0 },
        };
        assert!(matches!(
            read_sections(&data, &layout),
            Err(Error::SectionSizeWrap {
                section: SectionName::Espec,
                ..
            })
        ));
    }

    #[test]
    fn parse_maps_every_read_error_to_its_client_status() {
        // A file shorter than the header: the client's 22-byte read is clamped
        // and reported as status 14.
        for len in 0..HEADER_SIZE {
            let err = EncodingFile::parse(&REAL_HEADER[..len]).expect_err("short header");
            assert!(matches!(err, Error::HeaderShort));
            assert_eq!(err.status(), Some(14));
            assert_eq!(err.open_status(), Some(14));
        }

        // A header the client's parser rejects: status 8 at the parser, which
        // `EncodingHeaderRead` remaps to 14 before the reader sees it.
        let mut bad = REAL_HEADER.to_vec();
        bad[2] = 2;
        let err = EncodingFile::parse(&bad).expect_err("bad version");
        assert!(matches!(err, Error::HeaderRejected));
        assert_eq!(err.status(), Some(8));
        assert_eq!(err.open_status(), Some(14));

        // A valid header with a section past the end: the client's range gate
        // fails the request; the completion finds no result and reports 3.
        let mut short = REAL_HEADER.to_vec();
        short.resize(HEADER_SIZE, 0);
        let err = EncodingFile::parse(&short).expect_err("espec past the end");
        assert!(matches!(
            err,
            Error::SectionOutOfRange {
                section: SectionName::Espec,
                ..
            }
        ));
        assert_eq!(err.status(), Some(3));
        assert_eq!(err.open_status(), Some(3));

        // A header that fits but is one byte short of the last section: the
        // client clamps and reports status 14.
        let end = 111_058_161usize;
        let mut truncated = vec![0u8; end - 1];
        truncated[..HEADER_SIZE].copy_from_slice(&REAL_HEADER);
        let err = EncodingFile::parse(&truncated).expect_err("last section short");
        assert!(matches!(
            err,
            Error::SectionShort {
                section: SectionName::EkeyPageBodies,
                ..
            }
        ));
        assert_eq!(err.status(), Some(14));
        assert_eq!(err.open_status(), Some(14));
    }

    fn key(byte: u8) -> [u8; 16] {
        [byte; 16]
    }

    fn page_entry(first: [u8; 16], hash: u8) -> [u8; PAGE_INDEX_ENTRY] {
        let mut out = [hash; PAGE_INDEX_ENTRY];
        out[..16].copy_from_slice(&first);
        out
    }

    /// A three-page file that exercises every lookup branch: exact and
    /// between first keys, a count-2 record, a low-byte zero terminator with a
    /// nonzero high byte, a page hash that is deliberately not the body's MD5,
    /// and an ekey size whose fifth byte is not part of the client's value.
    fn lookup_file() -> Vec<u8> {
        let mut h = header();
        h[5..7].copy_from_slice(&4u16.to_be_bytes());
        h[7..9].copy_from_slice(&4u16.to_be_bytes());
        h[9..13].copy_from_slice(&3u32.to_be_bytes());
        h[13..17].copy_from_slice(&3u32.to_be_bytes());
        h[18..22].copy_from_slice(&2u32.to_be_bytes());

        let ck_keys = [key(0x10), key(0x40), key(0x80)];
        let es_keys = [key(0xA0), key(0xB0), key(0xC0)];

        let ck_index = HEADER_SIZE + 2;
        let ck_bodies = ck_index + 3 * PAGE_INDEX_ENTRY;
        let es_index = ck_bodies + 3 * 4096;
        let es_bodies = es_index + 3 * PAGE_INDEX_ENTRY;
        let mut out = vec![0u8; es_bodies + 3 * 4096];
        out[..HEADER_SIZE].copy_from_slice(&h);
        out[HEADER_SIZE] = b'a';

        for (page, first) in ck_keys.iter().enumerate() {
            // The hash bytes are deliberately wrong; the client's lookup never
            // reads them.
            let entry = page_entry(*first, 0xEE);
            let at = ck_index + page * PAGE_INDEX_ENTRY;
            out[at..at + PAGE_INDEX_ENTRY].copy_from_slice(&entry);
        }
        for (page, first) in es_keys.iter().enumerate() {
            let entry = page_entry(*first, 0xDD);
            let at = es_index + page * PAGE_INDEX_ENTRY;
            out[at..at + PAGE_INDEX_ENTRY].copy_from_slice(&entry);
        }

        let mut ck_page = [0u8; 4096];
        let mut p = 0usize;
        // Page 0: key 0x10 with one ekey, then key 0x20 with two.
        ck_page[p] = 1;
        ck_page[p + 2..p + 6].copy_from_slice(&0x1122_3344u32.to_be_bytes());
        ck_page[p + 6..p + 22].copy_from_slice(&key(0x10));
        ck_page[p + 22..p + 38].copy_from_slice(&key(0xA0));
        p += 38;
        ck_page[p] = 2;
        ck_page[p + 2..p + 6].copy_from_slice(&0x5566_7788u32.to_be_bytes());
        ck_page[p + 6..p + 22].copy_from_slice(&key(0x20));
        ck_page[p + 22..p + 38].copy_from_slice(&key(0xB0));
        ck_page[p + 38..p + 54].copy_from_slice(&key(0xB1));
        out[ck_bodies..ck_bodies + 4096].copy_from_slice(&ck_page);

        let mut ck_page = [0u8; 4096];
        ck_page[0] = 1;
        ck_page[2..6].copy_from_slice(&7u32.to_be_bytes());
        ck_page[6..22].copy_from_slice(&key(0x40));
        ck_page[22..38].copy_from_slice(&key(0xC0));
        out[ck_bodies + 4096..ck_bodies + 8192].copy_from_slice(&ck_page);

        let mut ck_page = [0u8; 4096];
        ck_page[0] = 1;
        ck_page[2..6].copy_from_slice(&9u32.to_be_bytes());
        ck_page[6..22].copy_from_slice(&key(0x80));
        ck_page[22..38].copy_from_slice(&key(0xD0));
        // Low byte zero, high byte one: the client's scan stops here and never
        // reaches the 0x81 record placed after it.
        ck_page[38] = 0;
        ck_page[39] = 1;
        ck_page[40] = 1;
        ck_page[42..46].copy_from_slice(&11u32.to_be_bytes());
        ck_page[46..62].copy_from_slice(&key(0x81));
        ck_page[62..78].copy_from_slice(&key(0xD1));
        out[ck_bodies + 8192..ck_bodies + 12288].copy_from_slice(&ck_page);

        let mut es_page = [0u8; 4096];
        es_page[..16].copy_from_slice(&key(0xA0));
        es_page[16..20].copy_from_slice(&0u32.to_be_bytes());
        // The u40's high byte is 0x77 and is not part of the client's 32-bit
        // value; the low four bytes are 0x00000102.
        es_page[20..25].copy_from_slice(&[0x77, 0x00, 0x00, 0x01, 0x02]);
        out[es_bodies..es_bodies + 4096].copy_from_slice(&es_page);

        let mut es_page = [0u8; 4096];
        es_page[..16].copy_from_slice(&key(0xB0));
        es_page[16..20].copy_from_slice(&1u32.to_be_bytes());
        es_page[20..25].copy_from_slice(&[0, 0, 0, 0, 3]);
        es_page[25..41].copy_from_slice(&key(0xB1));
        es_page[41..45].copy_from_slice(&2u32.to_be_bytes());
        es_page[45..50].copy_from_slice(&[0, 0, 0, 0, 4]);
        out[es_bodies + 4096..es_bodies + 8192].copy_from_slice(&es_page);

        let mut es_page = [0u8; 4096];
        es_page[..16].copy_from_slice(&key(0xC0));
        es_page[16..20].copy_from_slice(&3u32.to_be_bytes());
        es_page[20..25].copy_from_slice(&[0, 0, 0, 0, 5]);
        es_page[25..29].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        out[es_bodies + 8192..es_bodies + 12288].copy_from_slice(&es_page);

        out
    }

    #[test]
    fn page_search_uses_the_clients_midpoint_and_boundary_rule() {
        let index = [
            page_entry(key(0x10), 0),
            page_entry(key(0x40), 0),
            page_entry(key(0x80), 0),
        ]
        .concat();

        assert_eq!(page_index_search(&index, 0, 16, &key(0x00), 16), u32::MAX);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x0F), 16), u32::MAX);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x10), 16), 0);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x11), 16), 0);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x3F), 16), 0);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x40), 16), 1);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x7F), 16), 1);
        assert_eq!(page_index_search(&index, 3, 16, &key(0x80), 16), 2);
        assert_eq!(page_index_search(&index, 3, 16, &key(0xFF), 16), 2);

        // A zero compare length always compares equal, so the first midpoint
        // wins: page 1 of three, not page 0.
        assert_eq!(page_index_search(&index, 3, 16, &key(0xFF), 0), 1);
        // One page: below is the widening miss, exact is page 0, above is 0.
        assert_eq!(page_index_search(&index, 1, 16, &key(0x0F), 16), u32::MAX);
        assert_eq!(page_index_search(&index, 1, 16, &key(0x10), 16), 0);
        assert_eq!(page_index_search(&index, 1, 16, &key(0xFF), 16), 0);

        // The hash bytes are not compared: flipping them changes nothing.
        let mut changed = index.clone();
        changed[16] = 0xAA;
        changed[48] = 0xBB;
        assert_eq!(page_index_search(&changed, 3, 16, &key(0x40), 16), 1);
    }

    #[test]
    fn page_search_compares_the_requested_key_length() {
        let mut index = [0u8; 2 * PAGE_INDEX_ENTRY];
        index[0] = 0x10;
        index[16] = 0x20;
        index[PAGE_INDEX_ENTRY] = 0x20;
        index[PAGE_INDEX_ENTRY + 1] = 0x01;
        index[PAGE_INDEX_ENTRY + 16] = 0x30;

        // One byte: the second page's first key 0x20 matches exactly, even
        // though the full 16-byte key would differ at byte 1.
        let mut probe = key(0x20);
        probe[1] = 0x09;
        assert_eq!(page_index_search(&index, 2, 1, &probe, 1), 1);
        // Two bytes: the same probe is above 0x2001, so the second page is the
        // last first key that does not exceed it.
        assert_eq!(page_index_search(&index, 2, 1, &probe, 2), 1);
        assert_eq!(page_index_search(&index, 2, 1, &probe, 0), 1);
    }

    #[test]
    fn page_lookup_finds_first_middle_and_last_records() {
        let file = lookup_file();
        let encoding = EncodingFile::parse(&file).expect("synthetic lookup file");

        assert_eq!(encoding.ekeys_for_ckey(&key(0x10)), vec![key(0xA0)]);
        assert_eq!(
            encoding.ekeys_for_ckey(&key(0x20)),
            vec![key(0xB0), key(0xB1)]
        );
        assert_eq!(encoding.ekeys_for_ckey(&key(0x40)), vec![key(0xC0)]);
        assert_eq!(encoding.ekeys_for_ckey(&key(0x80)), vec![key(0xD0)]);
    }

    #[test]
    fn page_lookup_misses_below_between_and_after() {
        let file = lookup_file();
        let encoding = EncodingFile::parse(&file).expect("synthetic lookup file");

        assert!(encoding.ekeys_for_ckey(&key(0x0F)).is_empty());
        assert!(encoding.ekeys_for_ckey(&key(0x21)).is_empty());
        assert!(encoding.ekeys_for_ckey(&key(0x7F)).is_empty());
        assert!(encoding.ekeys_for_ckey(&key(0x81)).is_empty());
        assert!(encoding.ekeys_for_ckey(&key(0xFF)).is_empty());
    }

    #[test]
    fn page_lookup_stops_on_a_zero_low_count_byte() {
        let file = lookup_file();
        let encoding = EncodingFile::parse(&file).expect("synthetic lookup file");
        // The 0x81 record exists in the page bytes but sits after a record
        // whose count's low byte is zero; the client stops at that byte.
        assert!(encoding.ekeys_for_ckey(&key(0x81)).is_empty());
    }

    #[test]
    fn ekey_lookup_finds_the_espec_record_and_ignores_the_fifth_size_byte() {
        let file = lookup_file();
        let encoding = EncodingFile::parse(&file).expect("synthetic lookup file");

        assert_eq!(encoding.stored_size(&key(0xA0)), Some(0x0000_0102));
        assert_eq!(encoding.stored_size(&key(0xB0)), Some(3));
        assert_eq!(encoding.stored_size(&key(0xB1)), Some(4));
        assert_eq!(encoding.stored_size(&key(0xC0)), Some(5));
        assert_eq!(encoding.stored_size(&key(0x9F)), None);
        assert_eq!(encoding.stored_size(&key(0xD0)), None);

        assert_eq!(encoding.decoded_size(&key(0x10)), Some(0x1122_3344));
        assert_eq!(encoding.decoded_size(&key(0x20)), Some(0x5566_7788));
        assert_eq!(encoding.decoded_size(&key(0x40)), Some(7));
        assert_eq!(encoding.decoded_size(&key(0x80)), Some(9));
        assert_eq!(encoding.decoded_size(&key(0x0F)), None);
    }

    #[test]
    fn page_lookup_ignores_a_wrong_page_hash() {
        // lookup_file writes 0xEE/0xDD into every index hash; the real MD5 is
        // never checked by the client's lookup and must not be checked here.
        let file = lookup_file();
        let encoding = EncodingFile::parse(&file).expect("synthetic lookup file");
        assert_eq!(encoding.ekeys_for_ckey(&key(0x40)), vec![key(0xC0)]);
    }

    #[test]
    fn page_reader_refuses_widths_it_cannot_walk() {
        // The client's header parser accepts 1..=16. This reader is fixed at
        // 16-byte keys, so the wrapper accepts these bytes while the record
        // reader must say why it cannot use them. The section reader itself
        // follows the client's `width + 16` index stride for any width.
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

    #[test]
    fn parse_rejects_an_espec_block_without_a_trailing_nul() {
        let mut file = minimal_file();
        file[HEADER_SIZE + 1] = b'x';
        let err = EncodingFile::parse(&file).expect_err("espec tail");
        assert!(matches!(err, Error::EspecNotTerminated));
        assert_eq!(err.status(), Some(8));
        assert_eq!(err.open_status(), Some(8));
    }

    #[test]
    fn espec_entry_count_counts_the_nul_bytes() {
        // The header's espec size is the big-endian u32 at 18..22. Build a
        // file with a seven-byte block `a\0bb\0c\0` (three strings) and the
        // minimal one-page sections after it.
        let mut full = vec![0u8; HEADER_SIZE + 7];
        full[..HEADER_SIZE].copy_from_slice(&header());
        full[18..22].copy_from_slice(&7u32.to_be_bytes());
        full[HEADER_SIZE..].copy_from_slice(b"a\0bb\0c\0");
        let ck_index = HEADER_SIZE + 7;
        let ck_bodies = ck_index + PAGE_INDEX_ENTRY;
        let es_index = ck_bodies + 4096;
        let es_bodies = es_index + PAGE_INDEX_ENTRY;
        full.resize(es_bodies + 4096, 0);
        let parsed = EncodingFile::parse(&full).expect("valid synthetic file");
        assert_eq!(parsed.espec(), b"a\0bb\0c\0");
        assert_eq!(parsed.espec_entry_count(), 3);
        assert_eq!(parsed.espec_string_offsets(), &[0, 2, 5]);
    }

    /// `FUN_142498a70` (`0x142498a70`): the last-byte gate, the empty-block
    /// read at `base + 0xFFFF_FFFF`, the empty-string reject, the
    /// string-start appends at the running size, and the tightest no-grow
    /// capacity.
    #[test]
    fn espec_string_index_builds_the_client_index() {
        let mut index = [0xCCu64; 8];
        let mut size = 0u32;
        assert_eq!(
            espec_string_index(b"ab\0c\0d\0", 0x1000, 0, &mut index, &mut size, 8),
            0
        );
        assert_eq!(size, 3);
        assert_eq!(index[..3], [0x1000, 0x1003, 0x1005]);
        // Slots past the new size are untouched.
        assert_eq!(index[3], 0xCC);

        // An unterminated block returns 8 and writes nothing, even though
        // it contains a NUL: the last-byte gate runs first.
        let mut index = [0xCCu64; 8];
        let mut size = 0u32;
        assert_eq!(
            espec_string_index(b"ab\0cd", 0x1000, 0, &mut index, &mut size, 8),
            ESPEC_NOT_TERMINATED as u8
        );
        assert_eq!(size, 0);
        assert_eq!(index, [0xCCu64; 8]);

        // An empty string returns 8 at its NUL, before its append; appends
        // already made stay made. `a\0\0`: `a` is appended, then the empty
        // second string returns 8.
        let mut index = [0xCCu64; 8];
        let mut size = 0u32;
        assert_eq!(
            espec_string_index(b"a\0\0", 0x1000, 0, &mut index, &mut size, 8),
            ESPEC_NOT_TERMINATED as u8
        );
        assert_eq!(size, 1);
        assert_eq!(index[0], 0x1000);
        // A leading NUL is an empty first string: 8 with no writes at all.
        let mut index = [0xCCu64; 8];
        let mut size = 0u32;
        assert_eq!(
            espec_string_index(b"\0", 0x1000, 0, &mut index, &mut size, 8),
            ESPEC_NOT_TERMINATED as u8
        );
        assert_eq!(size, 0);
        assert_eq!(index, [0xCCu64; 8]);

        // An empty block: the last-byte read is at `base + 0xFFFF_FFFF`
        // (zero-extended `count - 1`), not at `base - 1`, so it returns 8
        // whatever byte sits at `base - 1`, with the size untouched.
        let mut index = [0xCCu64; 8];
        let mut size = 3u32;
        assert_eq!(
            espec_string_index(b"", 0x1000, 0, &mut index, &mut size, 8),
            ESPEC_NOT_TERMINATED as u8
        );
        assert_eq!(size, 3);
        assert_eq!(
            espec_string_index(b"", 0x1000, 0x41, &mut index, &mut size, 8),
            ESPEC_NOT_TERMINATED as u8
        );
        assert_eq!(size, 3);

        // Appends land at the incoming size, and capacity == the NUL count
        // is the tightest no-grow case (the reserve is a no-op and every
        // pre-append size is below capacity).
        let mut index = [0xCCu64; 4];
        let mut size = 1u32;
        assert_eq!(
            espec_string_index(b"a\0b\0c\0", 0x2000, 0, &mut index, &mut size, 4),
            0
        );
        assert_eq!(size, 4);
        assert_eq!(index, [0xCC, 0x2000, 0x2002, 0x2004]);
    }

    /// Both grow edges are the allocator-vtable path at `0x142388210`,
    /// outside this system; the function panics instead of inventing one.
    #[test]
    #[should_panic(expected = "vector grow")]
    fn espec_string_index_panics_on_the_reserve_grow() {
        let mut index = [0u64; 2];
        let mut size = 0u32;
        // Three NULs against a capacity of two: the reserve must grow.
        let _ = espec_string_index(b"a\0b\0c\0", 0x1000, 0, &mut index, &mut size, 2);
    }

    #[test]
    #[should_panic(expected = "vector grow")]
    fn espec_string_index_panics_on_the_append_grow() {
        let mut index = [0u64; 3];
        let mut size = 2u32;
        // The reserve passes (two NULs <= 3), but the second append finds
        // `size == capacity` and must grow.
        let _ = espec_string_index(b"a\0b\0", 0x1000, 0, &mut index, &mut size, 3);
    }
}

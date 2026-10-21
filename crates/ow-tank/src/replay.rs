//! The game's own replay container: `prp`, and the `phl` file that holds it.
//!
//! This is **not** our simulation trace replay. Nothing here re-runs our own
//! model; this is the container the game writes when it records a highlight, so
//! it is a recording of the *real* client. See `docs/formats/trace.md` for the
//! other thing, which is unrelated and easy to confuse on a grep.
//!
//! ## Where the files are, and the asset-type answer
//!
//! Replays are **not** in the game install and there is **no** asset type id for
//! them. OWLib's `AssetRepoType` enum (`TankTonka/Models/Common.cs`) has no
//! replay member, and OWLib itself does not look for replays anywhere in the
//! content manifest — it walks the *user's application-data folder*
//! (`DataTool/ToolLogic/List/Misc/ListHighlights.cs:74-79`):
//!
//! ```text
//! %LOCALAPPDATA%\Blizzard Entertainment\Overwatch\<account>\Highlights\<uuid>
//! ```
//!
//! with a `PTR\` level inserted on the test server. On the machine this was
//! written for there are no `*.prp` files anywhere under the install, and the
//! Highlights folders hold 15 extension-less files whose first three bytes are
//! `phl` — the *highlight* container, magic `p`,`h`,`l` ("Player HighLight").
//! Each `phl` file ends with a `prp` block, which is the thing worth reading:
//! it names the map, the game mode, the build and the hero roster.
//!
//! ## What is verified against those files, and what is not
//!
//! Verified on all 15 files of the Highlights folders (*install* — they are
//! user data rather than install data, but they are the real client's output):
//!
//! * the `prp` magic, the format-version byte at `+0x03`, the build number at
//!   `+0x04`, the map GUID at `+0x08` and the game-mode GUID at `+0x10` — the
//!   map and game mode agree with the enclosing `phl`'s own two GUIDs in every
//!   file, which is what pins those two offsets;
//! * the `int32` immediately before the `prp` magic is the block's own length,
//!   exactly, in every file (`find_replay` checks this rather than trusting it);
//! * the `phl` prefix through offset `0x6C` as [`Container`] declares it:
//!   `data_length` equals the file length minus 40, `build_number` is the
//!   account's own build folder, `player_id` is the account folder name, and
//!   the map/game-mode GUIDs at `0x5C`/`0x64` match the embedded `prp`;
//! * a Zstd frame begins where [`first_zstd_frame`] says it does, and the frame
//!   **does not record its own uncompressed size** — the frame header's content
//!   size field is absent in 15/15 — so no reader without a Zstd decoder can
//!   report the decompressed length. That is a property of the writer, not of
//!   this crate.
//!
//! **Not verified**: everything from `+0x18` on — the three unknown scalars, the
//! 32-byte map checksum, the params-block length, and the params block and hero
//! roster that follow. [`Replay::parse`] reads them exactly as OWLib declares
//! them (`tePlayerReplay.cs:13-35`, `HeroData.cs`, `tePlayerHighlight.cs:59-88`),
//! but on the files in this install that declaration does not line up: the
//! declared params block never ends where the Zstd frame begins, and
//! `params_block_length` reads as zero in every file. So `Replay::parse` will
//! usually *fail* on real data rather than return a plausible-looking roster.
//! That is deliberate. See `docs/formats/replay.md` for the measurements, and
//! `Replay::payload_offset` against [`first_zstd_frame`] for the check itself.
//!
//! ## The payload is not decoded
//!
//! The tail of every `prp` block is a Zstd frame holding the recorded game
//! state — where every player was on every tick. This crate carries no Zstd
//! decoder and is not allowed to add one, so [`ZstdBlock::decode`] always
//! returns [`Error::PayloadNotDecoded`]. The block reports its compressed
//! length and, where the writer declared one, its uncompressed length; it never
//! invents either. OWLib does not decode it either: its own decompression call
//! is commented out (`TankLib/Helpers/DataSerializer/Logical.cs:196`), so the
//! parity claim here is narrow — we both stop at the same place.

use std::fmt;

/// The block magic: `p`, `r`, `p` — "Player RePlay".
pub const MAGIC: [u8; 3] = *b"prp";

/// The enclosing container's magic: `p`, `h`, `l` — "Player HighLight".
pub const CONTAINER_MAGIC: [u8; 3] = *b"phl";

/// Zstd's frame magic, little-endian on disk: `28 B5 2F FD`.
pub const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Size of the `prp` prefix that is verified against real files: magic (3),
/// format version (1), build number (4), map (8), game mode (8).
pub const PREFIX_LEN: usize = 24;

/// Size of the `phl` header prefix this reader reads: `Container`'s fields end
/// here.
pub const CONTAINER_PREFIX_LEN: usize = 108;

/// Block-relative offset of the params block: the block-length field is an
/// `int32` at `+0x41`, so the block itself starts at `+0x45`.
pub const PARAMS_OFFSET: usize = 69;

/// Block-relative offset of the params block's hero count: five fixed scalars
/// (two `u32` then three `u64`) sit between the block's start and it.
pub const HERO_COUNT_OFFSET: usize = PARAMS_OFFSET + 4 * 2 + 8 * 3;

/// Longest array element count this reader will accept for a counted array,
/// as a sanity ceiling on a field the format does not otherwise bound. A count
/// above it means the bytes are not what they claim to be, not that the replay
/// really had that many heroes.
const MAX_ARRAY_LEN: usize = 4096;

#[derive(Debug)]
pub enum Error {
    /// The buffer does not begin with the magic this structure requires.
    BadMagic {
        what: &'static str,
        expected: [u8; 3],
        found: [u8; 3],
    },
    /// A read ran past the end of the buffer. Never a partial read: every field
    /// is bounds-checked before it is taken.
    Truncated {
        what: &'static str,
        need: usize,
        have: usize,
    },
    /// A length field is negative.
    NegativeLength { what: &'static str, value: i64 },
    /// A count field is negative.
    NegativeCount { what: &'static str, value: i32 },
    /// A count field is implausibly large for the format.
    CountTooLarge { what: &'static str, value: i32 },
    /// The length prefix that precedes a `prp` block does not match the bytes
    /// that follow it.
    LengthPrefixMismatch { declared: i32, actual: usize },
    /// The payload is a Zstd frame that this reader does not decode.
    PayloadNotDecoded {
        compressed_len: usize,
        declared_uncompressed_len: Option<u64>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic {
                what,
                expected,
                found,
            } => write!(
                f,
                "not a {what}: magic is {:02x?}, expected {:02x?}",
                found, expected
            ),
            Error::Truncated { what, need, have } => {
                write!(f, "{what}: needs {need} bytes, {have} left")
            }
            Error::NegativeLength { what, value } => {
                write!(f, "{what}: negative length {value}")
            }
            Error::NegativeCount { what, value } => write!(f, "{what}: negative count {value}"),
            Error::CountTooLarge { what, value } => {
                write!(f, "{what}: count {value} is implausible for this format")
            }
            Error::LengthPrefixMismatch { declared, actual } => write!(
                f,
                "replay block length prefix says {declared} bytes, {actual} follow it"
            ),
            Error::PayloadNotDecoded {
                compressed_len,
                declared_uncompressed_len,
            } => write!(
                f,
                "replay payload not decoded: {compressed_len} compressed bytes, \
                 uncompressed length {} (this crate carries no Zstd decoder)",
                match declared_uncompressed_len {
                    Some(n) => format!("declared as {n}"),
                    None => "not recorded by the writer".to_string(),
                }
            ),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A bounds-checked little-endian reader. Every accessor checks first, so a
/// short buffer produces [`Error::Truncated`] instead of a partial value.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    end: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor {
            bytes,
            at: 0,
            end: bytes.len(),
        }
    }

    fn remaining(&self) -> usize {
        self.end - self.at
    }

    fn offset(&self) -> usize {
        self.at
    }

    /// Take `n` bytes, or fail with the field's name.
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Truncated {
                what,
                need: n,
                have: self.remaining(),
            });
        }
        let out = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(out)
    }

    /// A sub-reader over the next `n` bytes. The parent is advanced past them,
    /// so a nested structure cannot read into its sibling.
    fn sub(&mut self, n: usize, what: &'static str) -> Result<Cursor<'a>> {
        let bytes = self.take(n, what)?;
        Ok(Cursor {
            bytes,
            at: 0,
            end: n,
        })
    }

    fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }

    fn i32(&mut self, what: &'static str) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }

    fn u32(&mut self, what: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }

    fn u64(&mut self, what: &'static str) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8, what)?.try_into().unwrap()))
    }

    fn f32(&mut self, what: &'static str) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }

    /// Three floats.
    fn vec3(&mut self, what: &'static str) -> Result<Vec3> {
        Ok(Vec3 {
            x: self.f32(what)?,
            y: self.f32(what)?,
            z: self.f32(what)?,
        })
    }

    /// Sixteen raw bytes, read as a GUID's in-memory form rather than as text.
    fn uuid(&mut self, what: &'static str) -> Result<Uuid> {
        let mut raw = [0u8; 16];
        raw.copy_from_slice(self.take(16, what)?);
        Ok(Uuid { raw })
    }

    /// A UTF-8 string terminated by a zero byte, bounded by the cursor.
    fn null_string(&mut self, what: &'static str) -> Result<String> {
        let start = self.at;
        let end_of_run = self.end;
        let mut len = 0usize;
        while start + len < end_of_run && self.bytes[start + len] != 0 {
            len += 1;
        }
        if start + len >= end_of_run {
            return Err(Error::Truncated {
                what: "unterminated string",
                need: len + 1,
                have: end_of_run - start,
            });
        }
        let text = std::str::from_utf8(&self.bytes[start..start + len])
            .map_err(|_| Error::Truncated {
                what,
                need: len + 1,
                have: end_of_run - start,
            })?
            .to_string();
        self.at = start + len + 1;
        Ok(text)
    }

    /// An `i32` count followed by that many `u32`s. The count is validated
    /// before anything is read, and the whole run is bounds-checked first.
    fn u32_array(&mut self, what: &'static str) -> Result<Vec<u32>> {
        let count = self.i32(what)?;
        self.u32_array_of(count, what)
    }

    fn u32_array_of(&mut self, count: i32, what: &'static str) -> Result<Vec<u32>> {
        if count < 0 {
            return Err(Error::NegativeCount { what, value: count });
        }
        let count = count as usize;
        if count > MAX_ARRAY_LEN {
            return Err(Error::CountTooLarge {
                what,
                value: count as i32,
            });
        }
        // Check the whole run before taking any of it: a truncated array must
        // not yield a short list that looks complete.
        self.take(
            count.checked_mul(4).ok_or(Error::CountTooLarge {
                what,
                value: count as i32,
            })?,
            what,
        )?;
        let at = self.at - count * 4;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(u32::from_le_bytes(
                self.bytes[at + i * 4..at + i * 4 + 4].try_into().unwrap(),
            ));
        }
        self.at = at + count * 4;
        Ok(out)
    }

    /// An `i32` length that must be non-negative and fit in what is left.
    fn length(&mut self, what: &'static str) -> Result<usize> {
        let value = self.i32(what)?;
        if value < 0 {
            return Err(Error::NegativeLength {
                what,
                value: value as i64,
            });
        }
        let value = value as usize;
        if value > self.remaining() {
            return Err(Error::Truncated {
                what,
                need: value,
                have: self.remaining(),
            });
        }
        Ok(value)
    }

    /// An `i32` count of nested structures. Only the count's plausibility and
    /// the buffer's bounds can be checked here; each element checks itself.
    fn count(&mut self, what: &'static str) -> Result<usize> {
        let value = self.i32(what)?;
        if value < 0 {
            return Err(Error::NegativeCount { what, value });
        }
        let value = value as usize;
        if value > MAX_ARRAY_LEN {
            return Err(Error::CountTooLarge {
                what,
                value: value as i32,
            });
        }
        Ok(value)
    }
}

/// Three floats, as `teVec3` stores them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A GUID's raw 16 bytes. Not the hyphenated text form: the files store the
/// in-memory layout, so the first three groups are little-endian and the last
/// eight bytes are in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uuid {
    pub raw: [u8; 16],
}

impl Uuid {
    /// The hyphenated text form, in the byte order Windows' `Guid` prints: the
    /// first three groups are the leading `u32`/`u16`/`u16` read little-endian,
    /// and the last eight bytes are in order.
    pub fn to_hyphenated(self) -> String {
        let b = self.raw;
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u16::from_le_bytes([b[4], b[5]]),
            u16::from_le_bytes([b[6], b[7]]),
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15],
        )
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hyphenated())
    }
}

/// The 32 bytes of a replay checksum, printed as upper-case hex.
///
/// The reference computes it as HMAC-SHA256 over some input with a fixed 32-byte
/// key (`ReplayChecksum.cs:43-49`), which needs a SHA-2 implementation this crate
/// does not carry. So the bytes are exposed and the derivation is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Checksum(pub [u8; 32]);

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

/// The part of a `prp` header that is verified against real files.
///
/// Offsets are block-relative: `+0x03` version, `+0x04` build, `+0x08` map,
/// `+0x10` game mode. Nothing after this is verified — see the module comment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    /// `+0x03`. 11 in every file measured, across all three client builds.
    pub format_version: u8,
    /// `+0x04`. The client build that wrote the replay. Equal to the enclosing
    /// container's own build where both were written by the same build; where
    /// it is not, the replay is the newer of the two numbers.
    pub build_number: u32,
    /// `+0x08`. Type bits `0x080` as stored (tool type `0x002`), low 32 bits the map
    /// index. The reference rewrites the type to `0x0790_0000_0000_0000` before
    /// reading it as a `STUMapHeader`.
    pub map: u64,
    /// `+0x10`. Type bits `0x023` as stored, which de-mangles to the tooling's
    /// game-mode type `0x0C5`.
    pub game_mode: u64,
}

impl Header {
    /// Read the verified prefix, checking the magic first.
    pub fn read(block: &[u8]) -> Result<Header> {
        // The magic is checked before the length: a file that is not a replay at
        // all should say so, rather than complain about being short. Only a
        // buffer too short to hold the magic itself is "truncated".
        if block.len() < MAGIC.len() {
            return Err(Error::Truncated {
                what: "replay magic",
                need: MAGIC.len(),
                have: block.len(),
            });
        }
        if block[..3] != MAGIC {
            return Err(Error::BadMagic {
                what: "replay",
                expected: MAGIC,
                found: [block[0], block[1], block[2]],
            });
        }
        if block.len() < PREFIX_LEN {
            return Err(Error::Truncated {
                what: "replay prefix",
                need: PREFIX_LEN,
                have: block.len(),
            });
        }
        Ok(Header {
            format_version: block[3],
            build_number: u32::from_le_bytes(block[4..8].try_into().unwrap()),
            map: u64::from_le_bytes(block[8..16].try_into().unwrap()),
            game_mode: u64::from_le_bytes(block[16..24].try_into().unwrap()),
        })
    }
}

/// One hero's entry in the replay's roster, as `HeroData` declares it.
///
/// **Not verified against real files** — see the module comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Hero {
    pub skin_id: u32,
    pub weapon_skin_id: u32,
    /// Play-of-the-game animation, which is a cosmetic id rather than an
    /// animation asset in the reference's reading.
    pub potg_animation: u32,
    pub spray_ids: Vec<u32>,
    pub voice_line_ids: Vec<u32>,
    pub emote_ids: Vec<u32>,
    /// The reference labels this an educated guess: every hero has a cosmetic
    /// category holding exactly one cosmetic, which used to be true of weapon
    /// skins too.
    pub announcer_id: u32,
    /// Hero asset; the tooling's hero type is `0x075`.
    pub hero: u64,
}

/// The replay's params block, as `tePlayerReplay.ReplayParams` declares it.
///
/// **Not verified against real files** — see the module comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub start_frame: u32,
    pub end_frame: u32,
    pub expected_duration_ms: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub heroes: Vec<Hero>,
}

/// The highlight description that a `prp` may carry, gated on a bit of the
/// header's first unknown byte.
///
/// **Not verified against real files** — see the module comment.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightInfo {
    pub player_name: String,
    pub unknown_byte: u8,
    /// `0` top-five, `1` play-of-the-game, `4` manual, per the reference's enum.
    pub kind: u8,
    pub unknown1: u32,
    pub unknown2: u32,
    pub unknown3: f32,
    pub unknown4: f32,
    pub elevation: u32,
    /// `0` blue, `1` red, `4` free-for-all, per `TankLib`'s `TeamIndex`.
    pub team_index: i32,
    pub position: Vec3,
    pub direction: Vec3,
    pub up: Vec3,
    pub hero: u64,
    pub item_skin_key: u64,
    pub item_wskin_key: u64,
    pub team_a: u64,
    pub team_b: u64,
    pub highlight_intro: u64,
    pub highlight_type: u64,
    pub timestamp: u64,
    pub uuid: Uuid,
}

/// How the highlight type shows up in the client, per `tePlayerHighlight`.
pub mod highlight {
    /// Top-five section.
    pub const KIND_TOP_5: u8 = 0x0;
    /// Play of the game.
    pub const KIND_PLAY_OF_THE_GAME: u8 = 0x1;
    /// The 36 manual highlights.
    pub const KIND_MANUAL: u8 = 0x4;

    /// The header's first unknown byte carries this bit when a highlight
    /// description follows the params block.
    pub const HAS_HIGHLIGHT_INFO: u8 = 0x4;

    /// Interface flags (`HighlightUIFlags`).
    pub const FLAG_VIEWED: u32 = 0x4;
    pub const FLAG_EXPORTED: u32 = 0x8;
}

/// The recorded game state at the tail of the block: a Zstd frame, not decoded.
///
/// Its length is not stored anywhere in the file either; the frame runs to the
/// end of the block, which the enclosing container's length prefix fixes.
#[derive(Debug, Clone, PartialEq)]
pub struct ZstdBlock {
    /// The compressed bytes, exactly as stored.
    pub bytes: Vec<u8>,
    /// The frame's own declared uncompressed size, when the writer recorded one.
    ///
    /// The replays measured on this machine's Highlights folders do **not**: the
    /// Zstd frame header's content-size field is absent in 15/15, so there is
    /// nothing to report and this is `None`. Where a writer did record it, it is
    /// the size the decompressed buffer will have.
    pub declared_uncompressed_len: Option<u64>,
}

impl ZstdBlock {
    /// How many compressed bytes the block holds. Always known: it is the length
    /// of the tail.
    pub fn compressed_len(&self) -> usize {
        self.bytes.len()
    }

    /// The uncompressed length, if the writer declared one.
    pub fn uncompressed_len(&self) -> Option<u64> {
        self.declared_uncompressed_len
    }

    /// Whether the bytes really start with a Zstd frame magic.
    pub fn has_frame_magic(&self) -> bool {
        self.bytes.starts_with(&ZSTD_MAGIC)
    }

    /// Decompress the payload.
    ///
    /// This always fails. The payload is Zstd-compressed and this crate carries
    /// no Zstd decoder — adding one is out of scope for the asset layer — so
    /// the honest answer is a named error rather than a fabricated buffer. The
    /// reference implementation stops in the same place: its decompression call
    /// is commented out, so it hands back the compressed bytes too.
    pub fn decode(&self) -> Result<Vec<u8>> {
        Err(Error::PayloadNotDecoded {
            compressed_len: self.compressed_len(),
            declared_uncompressed_len: self.declared_uncompressed_len,
        })
    }
}

/// Read a Zstd frame header's declared content size, if it has one.
///
/// The frame header is: a 4-byte magic, a descriptor byte whose top two bits
/// give the width of the content-size field and whose bit 5 says whether the
/// window descriptor is present, an optional window byte, an optional dictionary
/// id, then the content size. The field is never absent in a well-formed frame —
/// it is at least one byte — but its width can be zero when the single-segment
/// bit is set and the writer chose the narrow form.
fn frame_content_size(frame: &[u8]) -> Option<u64> {
    if frame.len() < 6 || frame[..4] != ZSTD_MAGIC {
        return None;
    }
    let descriptor = frame[4];
    let content_size_flag = descriptor >> 6;
    let single_segment = (descriptor >> 5) & 1 == 1;
    let dictionary_id_flag = descriptor & 0x3;
    let mut at = 5usize;
    if !single_segment {
        at += 1; // window descriptor
    }
    at += [0usize, 1, 2, 4][dictionary_id_flag as usize];
    // A zero flag means a one-byte field when the frame is a single segment and
    // a missing field otherwise; a one-byte field stores the real size minus
    // 256, the other widths store it verbatim.
    let width = match content_size_flag {
        0 => {
            if single_segment {
                1
            } else {
                return None;
            }
        }
        1 => 2,
        2 => 4,
        _ => 8,
    };
    if frame.len() < at + width {
        return None;
    }
    let mut value = 0u64;
    for (i, b) in frame[at..at + width].iter().enumerate() {
        value |= (*b as u64) << (8 * i);
    }
    if width == 2 {
        value += 256;
    }
    Some(value)
}

/// Where a `prp` block sits inside a buffer, and how long it claims to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// Block-relative offset of the `prp` magic.
    pub offset: usize,
    /// The block's length, taken from the `int32` that precedes the magic.
    pub length: usize,
}

/// Find the `prp` block in a `phl` file (or in a buffer that is itself a bare
/// `prp` block).
///
/// The `int32` immediately before the magic is the block's length; it is
/// checked against the bytes that actually follow and the search fails if they
/// disagree, rather than handing back a span that runs off the end. That check
/// holds on every file measured (see the module comment), which is why it is a
/// hard error and not a warning.
pub fn find_replay(bytes: &[u8]) -> Result<Block> {
    let mut search = 0usize;
    loop {
        let Some(found) = find_from(bytes, b"prp", search) else {
            return Err(Error::BadMagic {
                what: "replay",
                expected: MAGIC,
                found: first_three(bytes),
            });
        };
        // The magic has to be preceded by its own length, so the earliest a
        // block can start is offset 4.
        if found >= 4 {
            let declared = i32::from_le_bytes(bytes[found - 4..found].try_into().unwrap());
            if declared >= 0 {
                let actual = bytes.len() - found;
                if declared as usize == actual {
                    return Ok(Block {
                        offset: found,
                        length: actual,
                    });
                }
                return Err(Error::LengthPrefixMismatch { declared, actual });
            }
        }
        search = found + 1;
    }
}

/// The offset of the first Zstd frame magic in `bytes`.
///
/// On a `prp` block this is where the recorded game state starts, which is the
/// check that says whether the declared header layout consumed the right number
/// of bytes: compare it against [`Replay::payload_offset`].
pub fn first_zstd_frame(bytes: &[u8]) -> Option<usize> {
    find_from(bytes, &ZSTD_MAGIC, 0)
}

fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn first_three(bytes: &[u8]) -> [u8; 3] {
    let mut out = [0u8; 3];
    for (i, b) in bytes.iter().take(3).enumerate() {
        out[i] = *b;
    }
    out
}

/// The `phl` container's verified header prefix.
///
/// Only the fields through offset `0x6C` are read, and every one of them was
/// checked against the Highlights folders on this machine. What follows them —
/// the highlight descriptions, the hero roster and the filler records — is not
/// decoded here: the reference's own reader carries a note that it sometimes
/// reads too many filler records on these files
/// (`tePlayerHighlight.cs:106-110`), and it does not land on the `prp` block
/// either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Container {
    /// `+0x03`. 5 in every file measured.
    pub format_version: u8,
    /// `+0x04`, 32 bytes. Not the same value as the container's own checksum
    /// derivation would suggest; exposed, not verified.
    pub checksum: Checksum,
    /// `+0x24`. Equals the file length minus 40 in every file measured, which
    /// is what fixes this header's prefix at 40 bytes.
    pub data_length: u32,
    /// `+0x48`. Distinct from the build number and smaller; the reference calls
    /// it a replay version.
    pub replay_version: u32,
    /// `+0x4C`. The build of the client that wrote the container. This is the
    /// account's own build folder on disk.
    pub build_number: u32,
    /// `+0x50`. The account id, which is the account folder's name.
    pub player_id: u64,
    /// `+0x58`. See [`highlight::FLAG_VIEWED`] and friends.
    pub flags: u32,
    /// `+0x5C`. The map, same shape as the embedded replay's; they agree in every
    /// file measured.
    pub map: u64,
    /// `+0x64`. The game mode, likewise.
    pub game_mode: u64,
    /// `+0x28`, 32 bytes, the reference's four unnamed 64-bit fields.
    pub unknown: [u64; 4],
}

impl Container {
    pub fn read(bytes: &[u8]) -> Result<Container> {
        // Magic before length, as in `Header::read`: a file that is not a `phl`
        // says so instead of reporting itself as too short.
        if bytes.len() < CONTAINER_MAGIC.len() {
            return Err(Error::Truncated {
                what: "highlight container magic",
                need: CONTAINER_MAGIC.len(),
                have: bytes.len(),
            });
        }
        if bytes[..3] != CONTAINER_MAGIC {
            return Err(Error::BadMagic {
                what: "highlight container",
                expected: CONTAINER_MAGIC,
                found: [bytes[0], bytes[1], bytes[2]],
            });
        }
        if bytes.len() < CONTAINER_PREFIX_LEN {
            return Err(Error::Truncated {
                what: "highlight container prefix",
                need: CONTAINER_PREFIX_LEN,
                have: bytes.len(),
            });
        }
        let mut c = Cursor::new(bytes);
        c.at = 3;
        let format_version = c.u8("format version")?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(c.take(32, "checksum")?);
        let checksum = Checksum(raw);
        let data_length = c.u32("data length")?;
        let unknown = [
            c.u64("unknown1")?,
            c.u64("unknown2")?,
            c.u64("unknown3")?,
            c.u64("unknown4")?,
        ];
        let replay_version = c.u32("replay version")?;
        let build_number = c.u32("build number")?;
        let player_id = c.u64("player id")?;
        let flags = c.u32("flags")?;
        let map = c.u64("map")?;
        let game_mode = c.u64("game mode")?;
        Ok(Container {
            format_version,
            checksum,
            data_length,
            replay_version,
            build_number,
            player_id,
            flags,
            map,
            game_mode,
            unknown,
        })
    }
}

/// A whole `prp` block, read exactly as `tePlayerReplay` declares it.
///
/// The declared layout is reproduced faithfully so the container's structure is
/// on the record and testable, but the tail of the header is **not verified**
/// against the files on this machine — see the module comment and
/// `docs/formats/replay.md`. [`Replay::parse`] fails on those files rather than
/// returning a roster it cannot justify.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub header: Header,
    /// `+0x18`. Bit 2 (`highlight::HAS_HIGHLIGHT_INFO`) says whether a
    /// [`HighlightInfo`] follows the params block.
    pub unknown1: u8,
    pub unknown2: u32,
    pub unknown3: u32,
    /// `+0x21`, 32 bytes.
    pub map_checksum: Checksum,
    /// `+0x41`. The declared size of the params block. Read and reported, never
    /// used to seek: the reference does not either.
    pub params_block_length: i32,
    pub params: Params,
    pub highlight_info: Option<HighlightInfo>,
    pub payload: ZstdBlock,
    /// Block-relative offset the declared layout puts the payload at. On a file
    /// whose declared layout is right this equals the offset
    /// [`first_zstd_frame`] reports for the block.
    pub payload_offset: usize,
}

impl Replay {
    /// Parse a `prp` block, following `tePlayerReplay`'s field order.
    ///
    /// Every length is validated before it is used and every read is
    /// bounds-checked, so a file that is not what it claims to be comes back as
    /// an error naming the field, never as a short roster.
    pub fn parse(block: &[u8]) -> Result<Replay> {
        let header = Header::read(block)?;
        let mut c = Cursor::new(block);
        c.at = PREFIX_LEN;

        let unknown1 = c.u8("unknown1")?;
        let unknown2 = c.u32("unknown2")?;
        let unknown3 = c.u32("unknown3")?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(c.take(32, "map checksum")?);
        let map_checksum = Checksum(raw);
        let params_block_length = c.i32("params block length")?;

        let params = Params::read(&mut c)?;
        let highlight_info = if unknown1 & highlight::HAS_HIGHLIGHT_INFO != 0 {
            // The declared length is honoured as a bound rather than read past:
            // a highlight description that runs outside its own length is a
            // file this layout does not describe.
            let length = c.length("highlight info length")?;
            Some(HighlightInfo::read(&mut c.sub(length, "highlight info")?)?)
        } else {
            None
        };

        let payload_offset = c.offset();
        let payload_bytes = c.take(c.remaining(), "replay payload")?;
        Ok(Replay {
            header,
            unknown1,
            unknown2,
            unknown3,
            map_checksum,
            params_block_length,
            params,
            highlight_info,
            payload: ZstdBlock {
                declared_uncompressed_len: frame_content_size(payload_bytes),
                bytes: payload_bytes.to_vec(),
            },
            payload_offset,
        })
    }
}

impl Params {
    fn read(c: &mut Cursor<'_>) -> Result<Params> {
        let start_frame = c.u32("start frame")?;
        let end_frame = c.u32("end frame")?;
        let expected_duration_ms = c.u64("expected duration")?;
        let start_ms = c.u64("start ms")?;
        let end_ms = c.u64("end ms")?;
        let count = c.count("hero count")?;
        let mut heroes = Vec::with_capacity(count);
        for _ in 0..count {
            heroes.push(Hero::read(c)?);
        }
        Ok(Params {
            start_frame,
            end_frame,
            expected_duration_ms,
            start_ms,
            end_ms,
            heroes,
        })
    }
}

impl Hero {
    fn read(c: &mut Cursor<'_>) -> Result<Hero> {
        let skin_id = c.u32("skin id")?;
        let weapon_skin_id = c.u32("weapon skin id")?;
        let potg_animation = c.u32("potg animation")?;
        let spray_ids = c.u32_array("spray count")?;
        let voice_line_ids = c.u32_array("voice line count")?;
        let emote_ids = c.u32_array("emote count")?;
        let announcer_id = c.u32("announcer id")?;
        let hero = c.u64("hero")?;
        Ok(Hero {
            skin_id,
            weapon_skin_id,
            potg_animation,
            spray_ids,
            voice_line_ids,
            emote_ids,
            announcer_id,
            hero,
        })
    }
}

impl HighlightInfo {
    fn read(c: &mut Cursor<'_>) -> Result<HighlightInfo> {
        let player_name = c.null_string("player name")?;
        let unknown_byte = c.u8("highlight unknown byte")?;
        let kind = c.u8("highlight kind")?;
        let unknown1 = c.u32("highlight unknown1")?;
        let unknown2 = c.u32("highlight unknown2")?;
        let unknown3 = c.f32("highlight unknown3")?;
        let unknown4 = c.f32("highlight unknown4")?;
        let elevation = c.u32("highlight elevation")?;
        let team_index = c.i32("highlight team index")?;
        let position = c.vec3("highlight position")?;
        let direction = c.vec3("highlight direction")?;
        let up = c.vec3("highlight up")?;
        let hero = c.u64("highlight hero")?;
        let item_skin_key = c.u64("highlight item skin")?;
        let item_wskin_key = c.u64("highlight item weapon skin")?;
        let team_a = c.u64("highlight team a")?;
        let team_b = c.u64("highlight team b")?;
        let highlight_intro = c.u64("highlight intro")?;
        let highlight_type = c.u64("highlight type")?;
        let timestamp = c.u64("highlight timestamp")?;
        let uuid = c.uuid("highlight uuid")?;
        Ok(HighlightInfo {
            player_name,
            unknown_byte,
            kind,
            unknown1,
            unknown2,
            unknown3,
            unknown4,
            elevation,
            team_index,
            position,
            direction,
            up,
            hero,
            item_skin_key,
            item_wskin_key,
            team_a,
            team_b,
            highlight_intro,
            highlight_type,
            timestamp,
            uuid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a synthetic `prp` block at the exact offsets the parser reads, so
    /// a drifted offset fails here instead of quietly agreeing.
    struct Builder {
        bytes: Vec<u8>,
        highlight: bool,
    }

    impl Builder {
        fn new(highlight: bool) -> Builder {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&MAGIC);
            bytes.push(11); // format version
            bytes.extend_from_slice(&154088u32.to_le_bytes()); // build
            bytes.extend_from_slice(&0x0800_0000_0000_069eu64.to_le_bytes()); // map
            bytes.extend_from_slice(&0x0230_0000_0000_0172u64.to_le_bytes()); // game mode
            bytes.push(if highlight {
                highlight::HAS_HIGHLIGHT_INFO
            } else {
                0
            });
            bytes.extend_from_slice(&7u32.to_le_bytes()); // unknown2
            bytes.extend_from_slice(&9u32.to_le_bytes()); // unknown3
            bytes.extend_from_slice(&[0xAB; 32]); // map checksum
                                                  // The declared params length is patched in once the body is built.
            debug_assert_eq!(bytes.len(), PARAMS_OFFSET - 4);
            bytes.extend_from_slice(&0i32.to_le_bytes());
            Builder { bytes, highlight }
        }

        fn params(mut self, start_frame: u32, heroes: &[(u32, &[u32])]) -> Builder {
            self.bytes.extend_from_slice(&start_frame.to_le_bytes());
            self.bytes
                .extend_from_slice(&(start_frame + 60).to_le_bytes());
            self.bytes.extend_from_slice(&18_000u64.to_le_bytes());
            self.bytes.extend_from_slice(&1_200u64.to_le_bytes());
            self.bytes.extend_from_slice(&19_200u64.to_le_bytes());
            self.bytes
                .extend_from_slice(&(heroes.len() as i32).to_le_bytes());
            for (hero_index, sprays) in heroes {
                self.bytes.extend_from_slice(&100u32.to_le_bytes()); // skin
                self.bytes.extend_from_slice(&200u32.to_le_bytes()); // weapon skin
                self.bytes.extend_from_slice(&300u32.to_le_bytes()); // potg
                for (count, values) in [
                    (sprays.len(), *sprays),
                    (0usize, &[] as &[u32]),
                    (0usize, &[] as &[u32]),
                ] {
                    self.bytes.extend_from_slice(&(count as i32).to_le_bytes());
                    for v in values {
                        self.bytes.extend_from_slice(&v.to_le_bytes());
                    }
                }
                self.bytes.extend_from_slice(&400u32.to_le_bytes()); // announcer
                self.bytes.extend_from_slice(
                    &(0x02E0_0000_0000_0000u64 + *hero_index as u64).to_le_bytes(),
                );
            }
            self
        }

        /// The highlight description is 142 bytes plus the name and its
        /// terminator; the reference stores its length in front of it.
        fn highlight(mut self, name: &str) -> Builder {
            assert!(self.highlight, "builder not set up for a highlight");
            let mut body = Vec::new();
            body.extend_from_slice(name.as_bytes());
            body.push(0);
            body.push(0); // unknown byte
            body.push(highlight::KIND_PLAY_OF_THE_GAME);
            body.extend_from_slice(&1u32.to_le_bytes());
            body.extend_from_slice(&2u32.to_le_bytes());
            body.extend_from_slice(&3.5f32.to_le_bytes());
            body.extend_from_slice(&4.5f32.to_le_bytes());
            body.extend_from_slice(&5u32.to_le_bytes());
            body.extend_from_slice(&1i32.to_le_bytes()); // team index: red
            for v in [1.0f32, 2.0, 3.0] {
                body.extend_from_slice(&v.to_le_bytes());
            }
            for v in [4.0f32, 5.0, 6.0] {
                body.extend_from_slice(&v.to_le_bytes());
            }
            for v in [7.0f32, 8.0, 9.0] {
                body.extend_from_slice(&v.to_le_bytes());
            }
            for v in [
                0x02E0_0000_0000_0011u64,
                0x0250_0000_0000_0012,
                0x0250_0000_0000_0013,
                0x0790_0000_0000_0014,
                0x0790_0000_0000_0015,
                0x0250_0000_0000_0016,
                0x0ABC_0000_0000_0017,
            ] {
                body.extend_from_slice(&v.to_le_bytes());
            }
            body.extend_from_slice(&123_456u64.to_le_bytes()); // timestamp
            body.extend_from_slice(&std::array::from_fn::<u8, 16, _>(|i| i as u8)); // uuid
            self.bytes
                .extend_from_slice(&(body.len() as i32).to_le_bytes());
            self.bytes.extend_from_slice(&body);
            self
        }

        fn payload(mut self, payload: &[u8]) -> Builder {
            self.bytes.extend_from_slice(payload);
            let params_len = self.params_len();
            let at = PARAMS_OFFSET - 4;
            self.bytes[at..at + 4].copy_from_slice(&(params_len as i32).to_le_bytes());
            self
        }

        /// The params block runs from just after its own length field to
        /// wherever the highlight description or the payload begins.
        fn params_len(&self) -> usize {
            // Walk the body to find where it ends, which is the only way to know
            // without re-implementing the reader here.
            let mut c = Cursor::new(&self.bytes);
            c.at = HERO_COUNT_OFFSET;
            let count = c.count("hero count").unwrap();
            for _ in 0..count {
                c.u32("").unwrap();
                c.u32("").unwrap();
                c.u32("").unwrap();
                for _ in 0..3 {
                    c.u32_array("").unwrap();
                }
                c.u32("").unwrap();
                c.u64("").unwrap();
            }
            c.offset() - PARAMS_OFFSET
        }
    }

    /// A Zstd frame header with no content-size field, as the measured replays
    /// carry: magic, a descriptor of zero (no content size, window descriptor
    /// present, no dictionary), then the window byte.
    fn zstd_frame_without_content_size() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&ZSTD_MAGIC);
        out.push(0x00); // descriptor
        out.push(0x40); // window descriptor
        out.extend_from_slice(&[0xAB; 16]); // block body, opaque
        out
    }

    #[test]
    fn the_verified_prefix_round_trips() {
        let block = Builder::new(false)
            .params(16, &[(2, &[0x11, 0x12]), (6, &[])])
            .payload(&zstd_frame_without_content_size())
            .bytes;
        let replay = Replay::parse(&block).expect("synthetic block parses");

        assert_eq!(replay.header.format_version, 11);
        assert_eq!(replay.header.build_number, 154088);
        assert_eq!(replay.header.map, 0x0800_0000_0000_069e);
        assert_eq!(replay.header.game_mode, 0x0230_0000_0000_0172);

        assert_eq!(replay.params.start_frame, 16);
        assert_eq!(replay.params.end_frame, 76);
        assert_eq!(replay.params.expected_duration_ms, 18_000);
        assert_eq!(replay.params.start_ms, 1_200);
        assert_eq!(replay.params.end_ms, 19_200);

        assert_eq!(replay.params.heroes.len(), 2);
        let first = &replay.params.heroes[0];
        assert_eq!(first.hero, 0x02E0_0000_0000_0002);
        assert_eq!(first.skin_id, 100);
        assert_eq!(first.weapon_skin_id, 200);
        assert_eq!(first.potg_animation, 300);
        assert_eq!(first.announcer_id, 400);
        assert_eq!(first.spray_ids, vec![0x11, 0x12]);
        assert!(first.voice_line_ids.is_empty());
        assert!(first.emote_ids.is_empty());
        assert_eq!(replay.params.heroes[1].hero, 0x02E0_0000_0000_0006);

        // The declared params length must agree with what the reader walked, or
        // the field means nothing.
        assert_eq!(
            replay.params_block_length as usize,
            replay.payload_offset - PARAMS_OFFSET
        );
        assert!(replay.highlight_info.is_none());

        // The payload is located exactly where the frame magic is.
        assert_eq!(
            replay.payload_offset,
            first_zstd_frame(&block).expect("the payload frame is in the block")
        );
        assert!(replay.payload.has_frame_magic());
        assert_eq!(replay.payload.compressed_len(), 22);
        assert_eq!(
            replay.payload.uncompressed_len(),
            None,
            "the synthetic frame declares no content size, like the real ones"
        );
    }

    #[test]
    fn the_highlight_block_is_read_when_the_flag_bit_is_set() {
        let block = Builder::new(true)
            .params(16, &[(2, &[])])
            .highlight("Rye")
            .payload(&zstd_frame_without_content_size())
            .bytes;
        let replay = Replay::parse(&block).expect("synthetic block parses");

        assert_eq!(replay.unknown1 & highlight::HAS_HIGHLIGHT_INFO, 0x4);
        let info = replay.highlight_info.expect("highlight info present");
        assert_eq!(info.player_name, "Rye");
        assert_eq!(info.kind, highlight::KIND_PLAY_OF_THE_GAME);
        assert_eq!(info.unknown1, 1);
        assert_eq!(info.unknown2, 2);
        assert_eq!(info.unknown3, 3.5);
        assert_eq!(info.unknown4, 4.5);
        assert_eq!(info.elevation, 5);
        assert_eq!(info.team_index, 1);
        assert_eq!(
            info.position,
            Vec3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        assert_eq!(
            info.direction,
            Vec3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            }
        );
        assert_eq!(
            info.up,
            Vec3 {
                x: 7.0,
                y: 8.0,
                z: 9.0
            }
        );
        assert_eq!(info.hero, 0x02E0_0000_0000_0011);
        assert_eq!(info.item_skin_key, 0x0250_0000_0000_0012);
        assert_eq!(info.item_wskin_key, 0x0250_0000_0000_0013);
        assert_eq!(info.team_a, 0x0790_0000_0000_0014);
        assert_eq!(info.team_b, 0x0790_0000_0000_0015);
        assert_eq!(info.highlight_intro, 0x0250_0000_0000_0016);
        assert_eq!(info.highlight_type, 0x0ABC_0000_0000_0017);
        assert_eq!(info.timestamp, 123_456);
        assert_eq!(info.uuid.raw, std::array::from_fn(|i| i as u8));
        assert_eq!(
            info.uuid.to_hyphenated(),
            "03020100-0504-0706-0809-0a0b0c0d0e0f"
        );
        assert_eq!(
            replay.payload_offset,
            first_zstd_frame(&block).expect("the payload frame is in the block")
        );
    }

    #[test]
    fn a_declared_content_size_is_read_from_the_frame_header() {
        // Descriptor 0b1110_0000: eight-byte content size, no window byte, no
        // dictionary, single segment.
        let mut frame = Vec::new();
        frame.extend_from_slice(&ZSTD_MAGIC);
        frame.push(0b1110_0000);
        frame.extend_from_slice(&65_536u64.to_le_bytes());
        frame.extend_from_slice(&[0u8; 8]);
        let block = Builder::new(false)
            .params(0, &[(2, &[])])
            .payload(&frame)
            .bytes;
        let replay = Replay::parse(&block).expect("synthetic block parses");
        assert_eq!(replay.payload.uncompressed_len(), Some(65_536));
        assert_eq!(
            replay.payload.compressed_len(),
            frame.len(),
            "the compressed length is the tail's own length"
        );
    }

    #[test]
    fn the_payload_reports_that_it_is_not_decoded() {
        let frame = zstd_frame_without_content_size();
        let block = Builder::new(false)
            .params(0, &[(2, &[])])
            .payload(&frame)
            .bytes;
        let replay = Replay::parse(&block).expect("synthetic block parses");

        // The honest answer is an error, not a buffer of zeros.
        match replay.payload.decode() {
            Err(Error::PayloadNotDecoded {
                compressed_len,
                declared_uncompressed_len,
            }) => {
                assert_eq!(compressed_len, frame.len());
                assert_eq!(declared_uncompressed_len, None);
            }
            other => panic!("decode returned {other:?}, expected PayloadNotDecoded"),
        }
        let rendered = replay.payload.decode().unwrap_err().to_string();
        assert!(
            rendered.contains("not recorded by the writer"),
            "the error must say the length is unknown rather than imply zero: {rendered}"
        );
    }

    #[test]
    fn a_bad_magic_is_refused_before_anything_is_read() {
        let mut block = Builder::new(false)
            .params(0, &[(2, &[])])
            .payload(&[])
            .bytes;
        block[1] = b'x';
        match Replay::parse(&block) {
            Err(Error::BadMagic {
                what,
                expected: MAGIC,
                found: [b'p', b'x', b'p'],
            }) => assert_eq!(what, "replay"),
            other => panic!("expected BadMagic, got {other:?}"),
        }
    }

    #[test]
    fn a_short_buffer_fails_naming_the_field_not_reading_past_the_end() {
        let block = Builder::new(false)
            .params(16, &[(2, &[0x11])])
            .payload(&[])
            .bytes;
        for cut in [0usize, 3, 20, 40, 70, 73, block.len() - 1] {
            let err = Replay::parse(&block[..cut]).expect_err("a cut block must fail");
            assert!(
                matches!(err, Error::Truncated { .. } | Error::BadMagic { .. }),
                "cut at {cut}: {err:?}"
            );
        }
    }

    #[test]
    fn a_short_buffer_with_the_wrong_magic_says_magic_not_length() {
        // Too short to hold the whole prefix, but long enough to hold a magic:
        // the magic is what is wrong, and that is the useful thing to say.
        let mut bytes = MAGIC.to_vec();
        bytes[1] = b'x';
        assert!(matches!(Replay::parse(&bytes), Err(Error::BadMagic { .. })));
        assert!(matches!(
            Container::read(&bytes),
            Err(Error::BadMagic { .. })
        ));
    }

    #[test]
    fn an_impossible_hero_count_is_refused_rather_than_allocated() {
        let block = Builder::new(false)
            .params(16, &[(2, &[])])
            .payload(&[])
            .bytes;
        // The hero count sits five scalars into the params block.
        let at = HERO_COUNT_OFFSET;
        let mut bad = block.clone();
        bad[at..at + 4].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(matches!(
            Replay::parse(&bad),
            Err(Error::NegativeCount {
                what: "hero count",
                ..
            })
        ));

        let mut huge = block.clone();
        huge[at..at + 4].copy_from_slice(&(i32::MAX).to_le_bytes());
        assert!(matches!(
            Replay::parse(&huge),
            Err(Error::CountTooLarge {
                what: "hero count",
                ..
            })
        ));
    }

    #[test]
    fn the_length_prefix_in_front_of_a_block_is_checked_not_trusted() {
        let mut container = Vec::new();
        container.extend_from_slice(&CONTAINER_MAGIC);
        container.push(5);
        container.extend_from_slice(&[0u8; 32]);
        container.extend_from_slice(&0u32.to_le_bytes());
        let block = Builder::new(false)
            .params(0, &[(2, &[])])
            .payload(&zstd_frame_without_content_size())
            .bytes;
        container.extend_from_slice(&(block.len() as i32).to_le_bytes());
        container.extend_from_slice(&block);

        let found = find_replay(&container).expect("the block is found");
        assert_eq!(found.offset, container.len() - block.len());
        assert_eq!(found.length, block.len());

        // A length that disagrees with the bytes after it is an error, not a
        // span that runs off the end.
        let mut wrong = container.clone();
        let at = found.offset - 4;
        wrong[at..at + 4].copy_from_slice(&((block.len() - 1) as i32).to_le_bytes());
        assert!(matches!(
            find_replay(&wrong),
            Err(Error::LengthPrefixMismatch { .. })
        ));
    }

    #[test]
    fn the_container_prefix_is_read_at_the_offsets_it_declares() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&CONTAINER_MAGIC);
        bytes.push(5); // format version
        bytes.extend_from_slice(&[0xCD; 32]); // checksum
        bytes.extend_from_slice(&0u32.to_le_bytes()); // data length
        for v in [11u64, 12, 13, 14] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&494u32.to_le_bytes()); // replay version
        bytes.extend_from_slice(&154088u32.to_le_bytes()); // build
        bytes.extend_from_slice(&471_143_481u64.to_le_bytes()); // player id
        bytes.extend_from_slice(&highlight::FLAG_VIEWED.to_le_bytes()); // flags
        bytes.extend_from_slice(&0x0800_0000_0000_069eu64.to_le_bytes()); // map
        bytes.extend_from_slice(&0x0230_0000_0000_0172u64.to_le_bytes()); // game mode
        assert_eq!(
            bytes.len(),
            CONTAINER_PREFIX_LEN,
            "the declared fields must add up to the declared prefix length"
        );

        let container = Container::read(&bytes).expect("container prefix parses");
        assert_eq!(container.format_version, 5);
        assert_eq!(container.checksum, Checksum([0xCD; 32]));
        assert_eq!(container.replay_version, 494);
        assert_eq!(container.build_number, 154088);
        assert_eq!(container.player_id, 471_143_481);
        assert_eq!(container.flags, highlight::FLAG_VIEWED);
        assert_eq!(container.map, 0x0800_0000_0000_069e);
        assert_eq!(container.game_mode, 0x0230_0000_0000_0172);
        assert_eq!(container.unknown, [11, 12, 13, 14]);
        assert_eq!(container.checksum.to_string().len(), 64);
    }

    #[test]
    fn an_undersized_container_is_refused() {
        let mut bytes = CONTAINER_MAGIC.to_vec();
        bytes.push(5);
        assert!(matches!(
            Container::read(&bytes),
            Err(Error::Truncated {
                what: "highlight container prefix",
                ..
            })
        ));
        assert!(matches!(
            Container::read(b"xxxx"),
            Err(Error::BadMagic { .. })
        ));
    }
}

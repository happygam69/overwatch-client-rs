//! The animation clip chunk (asset type 0x006): the container.
//!
//! The quotes below are the *meaning* of each field, and where it comes from:
//!
//! * the layout was read out of 184 shipped clips by differential analysis
//!   (`dev/clip_lab.py`, `docs/formats/animation.md` § "The clip chunk, read
//!   empirically") — the header's frame count and fps, the duration that equals
//!   frames/fps in every sample, and the ten-qword section table at `+0x30`;
//! * three header fields are *stored reciprocals*, read on clips of three sizes:
//!   `+0x18` = `1/duration`, `+0x1c` = the fps as a float, `+0x20` = `1/fps`;
//! * `+0x00` is a **family word**, not a content id: it partitions the corpus into
//!   layout families (see `docs/formats/animation.md` § "The corpus is several clip
//!   families"), so a record-level rule holds *per family*, never per format;
//! * the *reading* of those records is confirmed by the client's own dispatcher:
//!   a function that takes (clip object, record) and switches on the **kind byte
//!   at `record + 0xe`** across 31 cases, tail-jumping to a handler per kind
//!   (`FUN_7ff613ecf660`, live `0x7ff613ecf660` = v2b `0x7ff7c58df660` in the
//!   practice-range session). So a record is a *typed* entry: a kind byte plus
//!   per-kind payload.
//!
//! What is **not** here, and why: the *file-side record offsets*. An earlier version of this
//! module parsed a table of 32-byte records whose second word was `frames + 1`,
//! which held exactly for one rig family — and a second family measured afterwards
//! uses a **20-byte** stride with that word at irregular offsets, so the rule was
//! over-generalised and is gone. The record stride is rig-dependent, and the client
//! dispatcher switches on a *kind byte* at `record + 0xe` across 31 handlers, which
//! is what defines the real format. The dispatch map and the per-kind sample codecs
//! are now ported below ([`KIND_TABLE`], [`decode_track_sample`]), but the walk to a
//! record's file offset stays content-based (bone-table search) and family-gated
//! ([`record_stride_for_family`]) — never positional — until the cold-kind handlers
//! are read and a record table offset is verified per family.
//!
//! Nothing here reads game data at build or test time: the tests build synthetic
//! bytes, and the CLI takes a path to a file the operator extracted themselves.
//!
//! Ported 2026-10-17 (*reference* + *decompiled* + *install*; behaviour only,
//! no code or comments copied):
//!
//! * the **kind dispatch** behind the kind byte at `record + 0xe`
//!   (`FUN_7ff613ecf660`, live `0x7ff613ecf660` = v2b `0x7ff7c58df660`;
//!   live = v2b − `0x1B1A10000` in the practice-range session):
//!   [`KIND_TABLE`] names all 31 kinds with each handler's v2b address and
//!   residency (inline / resident / cold-unverified / default stub). Kinds
//!   6, 9, 11–14, 16, 17 and 20–30 share the default stub (v2b
//!   `0x7ff7c58df913`); kinds 1, 4, 5, 7, 8, 15, 19, 31 name a handler whose
//!   page is zero-filled in every standing capture and therefore decode as
//!   cold-unverified, never executed here.
//! * the **track codecs** the payloads use, ported from the reference
//!   (`E:/CASCReader/OWLib/TankLib/teAnimation.cs`: `ReadRotation` /
//!   `UnpackOldRotation` / `UnpackNewRotation` / `ReadPosition` / `ReadScale` /
//!   `ReadIndices`) and verified against the install (the pinned six-byte sample
//!   in `crates/ow-tank/tests/rotation_format.rs` decodes to `|q| = 1.000000`):
//!   [`decode_track_sample`] routes `TrackKind` + `TrackFlags` to the old 6-byte
//!   or new 8-byte rotation, the f16÷32 or f32 position, or the ÷1024 scale, and
//!   [`read_frame_indices`] ports the `u8`-vs-`i16` frame rule keyed on
//!   `InfoTableSize = int(fps × duration) + 1` with `frame = abs(index) % size`.
//! * **family-word gating** (*install*): [`record_stride_for_family`] returns the
//!   20-byte stride only for the two families it was measured on (`0x006d2000`,
//!   `0x00533c00`); every other family yields `None`, because the earlier 32-byte
//!   grid was an index-table false positive and no family is verified at 32 bytes.
//!   [`RECORD`] is retained for existing callers and documented as ungated;
//!   there is no 32-byte constant, because that grid was a false positive.
//! * a **bounded value search** (*install*): [`CHANNEL_VALUE_WINDOW`] caps the
//!   channel value hunt at 8 KB past the key list (measured coverage 65% there,
//!   against 73.1% unbounded — the cap trades the last 8 points for a bounded
//!   scan), and [`build_unit_sample_index`] prebuilds the candidate offsets once
//!   per body so the window costs one scan, not one per record.
//!   [`decode_channels`] keeps its shape and now delegates to
//!   [`decode_channels_with_window`].
//! * **slerp** (*published-check*: standard unit-quaternion interpolation, no game
//!   bytes involved): [`slerp_quat`] replaces the linear blend in
//!   [`Channel::rotation_at`] and [`BoneTrack::quat_at`]. Linear interpolation
//!   leaves the great arc — the midpoint stays unit here only because `w` is
//!   re-derived, while the angular velocity bunches at the ends — so the pose
//!   between keys eased rather than swept. Slerp travels at constant angular
//!   velocity with the double-cover sign flip.

use std::fmt;

use glam::{Quat, Vec3};

/// The ten-qword section table sits right after the fixed header.
pub const SECTION_TABLE: usize = 0x30;
/// Number of entries in it (the table ends where the body begins, at 0x80).
pub const SECTIONS: usize = 10;
/// The body starts at this offset in every sampled clip.
pub const BODY: usize = 0x80;
/// Records in the second section were once read as this wide. Retained for
/// existing callers, but **ungated**: no family is verified at 32 bytes (the
/// grid was an index-table false positive), so new code must use
/// [`record_stride_for_family`] and treat `None` as "do not walk".
pub const RECORD: usize = 32;

/// The fixed header of a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    /// `+0x00`: the **family word**. It is a discriminator, not a content id: it
    /// partitions the corpus into layout families (24 clips `0x00722000`, 12
    /// `0x006d2000`, 9 `0x00d02000`, … — and the clip whose graph is decoded in
    /// `docs/formats/animation.md` is the *singleton* `0x002a0604`), and clips
    /// that share it do **not** share a size or a record stride. Whether it splits
    /// into a class and a version is not established.
    pub family: u32,
    /// `+0x0c`: frame count.
    pub frames: u16,
    /// `+0x0e`: frames per second (30 in every sampled clip).
    pub fps: u16,
    /// `+0x14`: seconds, equal to `frames / fps` in every sampled clip.
    pub duration: f32,
    /// `+0x18`: `1 / duration`, *stored* — read on three clips of three sizes
    /// (0.5333/1.875, 1.5/0.6667, 3.0/0.3333).
    pub rate: f32,
    /// `+0x1c`: the frame rate again, as a float (30.0).
    pub fps_f32: f32,
    /// `+0x20`: `1 / fps`, the per-frame step in seconds (0.03333 at 30 fps).
    pub frame_period: f32,
    /// `+0x24`: a per-clip id.
    pub id: u32,
    /// `+0x28`: a small counter that runs within a family.
    pub slot: u32,
}

impl Header {
    pub fn read(bytes: &[u8]) -> Option<Header> {
        if bytes.len() < SECTION_TABLE {
            return None;
        }
        let u16_at = |o: usize| -> Option<u16> {
            bytes
                .get(o..o + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
        };
        let u32_at = |o: usize| -> Option<u32> {
            bytes
                .get(o..o + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let f32_at = |o: usize| -> Option<f32> {
            bytes
                .get(o..o + 4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        Some(Header {
            family: u32_at(0x00)?,
            frames: u16_at(0x0c)?,
            fps: u16_at(0x0e)?,
            duration: f32_at(0x14)?,
            rate: f32_at(0x18)?,
            fps_f32: f32_at(0x1c)?,
            frame_period: f32_at(0x20)?,
            id: u32_at(0x24)?,
            slot: u32_at(0x28)?,
        })
    }

    /// The duration the frame count and rate imply. The client's own duration is
    /// read from the file; this is the cross-check every sample passed.
    pub fn implied_duration(&self) -> f32 {
        if self.fps == 0 {
            0.0
        } else {
            self.frames as f32 / self.fps as f32
        }
    }

    /// `1 / duration` — the relation the corpus stores at `+0x18`.
    pub fn implied_rate(&self) -> f32 {
        if self.duration == 0.0 {
            0.0
        } else {
            1.0 / self.duration
        }
    }

    /// `1 / fps` — the relation the corpus stores at `+0x20`.
    pub fn implied_frame_period(&self) -> f32 {
        if self.fps == 0 {
            0.0
        } else {
            1.0 / self.fps as f32
        }
    }
}

/// The reference implementation's header, ported field for field.
///
/// **Source:** `E:/CASCReader/OWLib/TankLib/teAnimation.cs`, struct `AnimHeader`
/// (`[StructLayout(LayoutKind.Sequential, Pack = 4)]`), lines 33-99. Ported
/// verbatim and with the same `Pack = 4` layout so every offset matches, because
/// we expect to copy much more from that project and matching headers keep the
/// ports diffable and replaceable in one place.
///
/// ## Status: PRESUMED WRONG on this build — do not trust without checking
///
/// Still unproven as of 2026-10-17: the kind/codec port elsewhere in this
/// module re-derives the *decodes*, not this header. [`Header`] remains the
/// decoder's source.
///
/// Measured against three installed clips, the field OWLib calls `bone_count`
/// (offset **2**) holds **the top half of the 32-bit family word**, not a bone
/// count:
///
/// | clip | family | word at offset 2 | `family >> 16` |
/// |---|---|---|---|
/// | `0x0a0000000000005e` | `0x006d2000` | 109 | **109** |
/// | `0x0a0000000000c15a` | `0x00020400` | 2 | **2** |
/// | `0x0a00000000000050` | `0x00533c00` | 83 | **83** |
///
/// The struct is therefore misaligned for this build, which is also why its
/// `bone_list_offset` and `info_table_offset` both read **0** here, and why the
/// per-bone `InfoTable` walk it drives cannot be reached. **Our own [`Header`]
/// is the one that agrees with the bytes** (`frames == duration * fps` exactly
/// on all three clips, and the reference's own `int(fps * duration) + 1` frame
/// count agrees with it).
///
/// Kept, rather than deleted, for two reasons: the *decodes* in that project are
/// verified against our bytes and are worth porting verbatim, and a matching
/// header makes those ports comparable line for line. If a corrected header is
/// ever established, this struct is the single place to change it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceHeader {
    pub priority: u8,
    pub group: u8,
    pub bone_count: u16,
    pub flags: u16,
    pub unknown1: u16,
    pub unknown2: u32,
    pub unknown3: u16,
    pub unknown4: u16,
    pub unknown5: u32,
    /// Seconds. **Verified**: matches our `duration` at offset 0x14 on all
    /// three sampled clips.
    pub duration: f32,
    pub unknown6: f32,
    /// **Verified**: matches our `fps_f32` at offset 0x1c.
    pub fps: f32,
    pub unknown7: f32,
    pub unknown8: f32,
    /// 16-byte GUID; the reference types it `teResourceGUID` (file type 08F).
    pub effect: [u8; 16],
    pub effect_storage: i64,
    pub bone_list_offset: i64,
    pub unknown9: i64,
    pub unknown10: i64,
    pub unknown11: i64,
    pub unknown12: i64,
    pub unknown13: i64,
    /// "new animation param table, usually float tracks" (the reference's own
    /// comment).
    pub some_table_offset: i64,
    pub info_table_offset: i64,
    pub padding: i64,
}

impl ReferenceHeader {
    /// Size in bytes. `Pack = 4` with two `u8` at the front packs the pair into
    /// one 4-byte slot, giving 136.
    pub const SIZE: usize = 136;

    /// Read the reference's header layout from the start of the file.
    ///
    /// Returns `None` on a short buffer rather than panicking, matching
    /// [`Header::read`].
    pub fn read(bytes: &[u8]) -> Option<ReferenceHeader> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let u8_at = |o: usize| -> Option<u8> { bytes.get(o).copied() };
        let u16_at = |o: usize| -> Option<u16> {
            bytes
                .get(o..o + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
        };
        let u32_at = |o: usize| -> Option<u32> {
            bytes
                .get(o..o + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let i64_at = |o: usize| -> Option<i64> {
            bytes
                .get(o..o + 8)
                .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        };
        let f32_at = |o: usize| -> Option<f32> {
            bytes
                .get(o..o + 4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        Some(ReferenceHeader {
            priority: u8_at(0)?,
            group: u8_at(1)?,
            bone_count: u16_at(2)?,
            flags: u16_at(4)?,
            unknown1: u16_at(6)?,
            unknown2: u32_at(8)?,
            unknown3: u16_at(12)?,
            unknown4: u16_at(14)?,
            unknown5: u32_at(16)?,
            duration: f32_at(20)?,
            unknown6: f32_at(24)?,
            fps: f32_at(28)?,
            unknown7: f32_at(32)?,
            unknown8: f32_at(36)?,
            effect: bytes.get(40..56)?.try_into().ok()?,
            effect_storage: i64_at(56)?,
            bone_list_offset: i64_at(64)?,
            unknown9: i64_at(72)?,
            unknown10: i64_at(80)?,
            unknown11: i64_at(88)?,
            unknown12: i64_at(96)?,
            unknown13: i64_at(104)?,
            some_table_offset: i64_at(112)?,
            info_table_offset: i64_at(120)?,
            padding: i64_at(128)?,
        })
    }

    /// `int(FPS * Duration) + 1` — the reference's own frame count.
    ///
    /// **Verified** to agree with [`Header::frames`] on all three sampled clips
    /// (21, 170, 5), so this is the rule our decoders use.
    pub fn info_table_size(&self) -> usize {
        (self.fps * self.duration) as usize + 1
    }
}

/// Byte offset of the kind discriminator inside a graph record.
///
/// *Decompiled*: `FUN_7ff613ecf660` (live `0x7ff613ecf660` = v2b
/// `0x7ff7c58df660`; live = v2b − `0x1B1A10000`) reads
/// `byte [record + 0xe]` as an index into its 31-entry jump table.
pub const RECORD_KIND_OFFSET: usize = 0x0e;

/// Byte offset of the flags word inside a graph record.
///
/// *Decompiled*: the same dispatcher tests `byte [record + 0xc]` (bit 1 selects
/// inline vec4s at `record + 0x10..0x2f`) before tail-jumping to the handler.
pub const RECORD_FLAGS_OFFSET: usize = 0x0c;

/// The verified record stride: 20 bytes, for the measured families.
///
/// *Install*: confirmed for families `0x00533c00` and `0x006d2000`
/// (`docs/formats/animation.md` § "The record stride is 20 bytes"). Returned by
/// [`record_stride_for_family`]; never assumed for any other word.
pub const RECORD_STRIDE_20: usize = 20;

/// The record stride for a clip family, or `None` where it is not measured.
///
/// *Install*: only `0x006d2000` and `0x00533c00` are verified at 20 bytes. The
/// `0x002a0604` singleton, the first-person family `0x00020400`, and every other
/// word yield `None` — a record-level rule holds *per family*, never per format,
/// so an unmeasured family must not be walked. In particular nothing maps to 32:
/// the old 32-byte grid was an index table, not records.
pub fn record_stride_for_family(family: u32) -> Option<usize> {
    match family {
        0x006d_2000 | 0x0053_3c00 => Some(RECORD_STRIDE_20),
        _ => None,
    }
}

/// A parsed clip: the header, the first section, and the record table.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub header: Header,
    /// The ten section offsets, `-1`/`0` for absent.
    pub sections: [i64; SECTIONS],
    /// The second section's offset.
    pub second_at: usize,
    /// The first section as words — a per-rig list that is byte-identical across
    /// a clip family, read as ids with runs of consecutive values.
    pub first_section: Vec<u32>,
    /// Everything from the first section's end to the end of the file: the
    /// per-rig section, then the payload the records live in.
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipError {
    TooShort,
    TableOutOfRange,
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipError::TooShort => write!(f, "shorter than the clip header"),
            ClipError::TableOutOfRange => write!(f, "the second section starts outside the file"),
        }
    }
}

impl Clip {
    /// Parse a clip chunk. Only the fields the corpus verified are interpreted;
    /// the payload is carried through untouched.
    pub fn parse(bytes: &[u8]) -> Result<Clip, ClipError> {
        let header = Header::read(bytes).ok_or(ClipError::TooShort)?;
        let mut sections = [0i64; SECTIONS];
        for (i, s) in sections.iter_mut().enumerate() {
            let o = SECTION_TABLE + i * 8;
            if o + 8 > bytes.len() {
                return Err(ClipError::TooShort);
            }
            *s = i64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        }
        // The first section runs from the body start (BODY) to the next section;
        // the record table is the first section after it, and it begins with a
        // 32-byte prefix — the records follow that, which is why they are read
        // from `table_at + RECORD`.
        let in_file = |v: i64| usize::try_from(v).ok().filter(|o| *o < bytes.len());
        let offsets: Vec<usize> = sections
            .iter()
            .skip(2)
            .filter_map(|v| in_file(*v))
            .collect();
        let body_end = offsets
            .iter()
            .copied()
            .filter(|o| *o > BODY)
            .min()
            .unwrap_or(bytes.len());
        let mut first_section = Vec::new();
        let mut o = BODY;
        while o + 4 <= body_end.min(bytes.len()) {
            first_section.push(u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()));
            o += 4;
        }
        let second_at = offsets
            .iter()
            .copied()
            .find(|o| *o >= BODY)
            .ok_or(ClipError::TableOutOfRange)?;
        let body = bytes[body_end.min(bytes.len())..].to_vec();
        Ok(Clip {
            header,
            sections,
            second_at,
            first_section,
            body,
        })
    }
}

/// The client's own decode of a stored 16-bit value, read from its code.
///
/// `FUN_7ff614ee4130` (live `0x7ff614ee4130` = v2b `0x7ff7c58f4130`) decodes a
/// stored word as
///
/// ```text
/// (u16 - 0x8000) * 3.05176e-05 + 0.001
/// ```
///
/// where the scale is the **first entry of a table of powers of two** at live
/// `0x7ff6164f38f8` (2^-15, 2^-14, 2^-10, 2^-8, 1024, 4096 — read from the client's
/// data section, `2^-15 = 3.05176e-05`) and `0.001` is a separate constant at live
/// `0x7ff6162f3ae4`. The `+0.001` and the shape of the surrounding code (a duration
/// compared against a threshold of `0.004`, two terms combined by an operator byte)
/// say this particular use is a **time or duration** value with a one-millisecond
/// bias — it is *one* of the record kinds' semantics, not necessarily the rotation
/// decode.
///
/// What that means for this module: the *arithmetic* is read, but **where in a
/// record such a value sits is not**, so nothing here calls it on payload bytes.
/// It is exposed so the pass that reads the handlers has the decode at hand and can
/// cite it.
pub fn evaluator_time_value(raw: u16) -> f32 {
    const SCALE_2_POW_MINUS_15: f32 = 3.051_757_8e-5;
    const MILLISECOND_BIAS: f32 = 0.001;
    (raw as i32 - 0x8000) as f32 * SCALE_2_POW_MINUS_15 + MILLISECOND_BIAS
}

/// A bone index table: a `0`-terminated, strictly increasing list of 1-based
/// bone numbers marking which bones a channel drives. The list may be **sparse**
/// (a gap means the channel does not animate that bone) — see
/// [`find_bone_tables`] and the measurement behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneTable {
    /// File offset of the table (its first entry).
    pub offset: usize,
    /// Number of entries in the list.
    pub bone_count: u8,
    /// The bone numbers themselves, in stored order. For a dense list this is
    /// `1..=bone_count`; for a sparse one it is not.
    pub bones: Vec<u8>,
    /// File offset just past the list's `0` terminator, so a caller can carry on
    /// from the record without re-deriving it (a sparse list makes
    /// `offset + bone_count` wrong).
    pub end: usize,
}

/// A per-bone rotation triplet: three f32 storing a unit quaternion as
/// `(x, y, z)` with `w = sqrt(1 - x² - y² - z²)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneRotation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl BoneRotation {
    /// The derived `w` component of the unit quaternion.
    pub fn w(&self) -> f32 {
        let mag_sq = self.x * self.x + self.y * self.y + self.z * self.z;
        if mag_sq <= 1.0 {
            (1.0 - mag_sq).sqrt()
        } else {
            0.0
        }
    }

    /// The quaternion as `(x, y, z, w)`.
    pub fn quat(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.z, self.w())
    }
}

/// Find all bone index tables in the clip body.
///
/// A bone index table is a **`0`-terminated list of strictly increasing bone
/// numbers starting at 1**, of at least 3 entries. Measured on the installed
/// corpus, the list may be **sparse**: Tracer's first-person clip
/// `0x0a0000000000c15a` stores `01..07, 09..81, 83..170` — 168 entries with two
/// gaps — and contains **no** strictly-consecutive run of length ≥ 8, so the
/// earlier "run of `01 02 03 …`" rule rejected it outright and that clip decoded
/// to zero channels. Accepting a sparse, ascending, terminated list covers both
/// shapes: a strict `01 02 03 … N 00` run is the special case where no bone is
/// skipped.
///
/// `bone_count` is the number of *entries*; the bone numbers themselves are in
/// [`BoneTable::bones`], so a caller that needs the mapping reads it rather than
/// assuming `1..=count`.
pub fn find_bone_tables(body: &[u8]) -> Vec<BoneTable> {
    let mut tables = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        // a candidate must start a strictly increasing 1-based sequence
        if body[i] != 1 {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i;
        let mut last = 0u8;
        let mut count = 0usize;
        let mut bones: Vec<u8> = Vec::new();
        while j < body.len() {
            let b = body[j];
            if b == 0 {
                break; // terminator
            }
            // strictly increasing, and never below the previous entry
            if b <= last {
                break;
            }
            last = b;
            bones.push(b);
            count += 1;
            j += 1;
            // A list cannot be longer than 255 entries (it is 1-based bytes).
            if count > 255 {
                break;
            }
        }
        // Require a terminator, at least 3 entries, and no overrun.
        let terminated = j < body.len() && body[j] == 0;
        if terminated && count >= 3 {
            tables.push(BoneTable {
                offset: start,
                bone_count: count as u8,
                bones,
                end: j + 1,
            });
            i = j + 1;
        } else {
            i = start + 1;
        }
    }
    tables
}

/// A float data region: a maximal run of plausible f32 values, not a bone index
/// table. "Plausible" means finite, non-zero and within `[1e-7, 16]` in
/// magnitude — the range unit quaternions live in. Byte patterns such as
/// integer tables decode to f32s with absurd exponents and do not form runs.
/// An f32 component that is exactly `0.0` terminates a run (a rare case in
/// quaternion data; the region then splits, and pairing still reads the
/// correct phase from either side).
///
/// The run is *extended in 4-byte steps* from its start, so a region is
/// 4-float aligned by construction, and shifted-phase windows (which decode to
/// implausible magnitudes) cannot extend a region across real data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatRegion {
    pub offset: usize,
    pub byte_count: usize,
}

/// Minimum floats in a region: three floats is one bone's quaternion.
const MIN_REGION_BYTES: usize = 12;

/// Find all float data regions in the clip body.
pub fn find_float_regions(body: &[u8]) -> Vec<FloatRegion> {
    let bone_tables = find_bone_tables(body);
    let plausible = |b: &[u8]| -> bool {
        let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        v.is_finite() && v != 0.0 && v.abs() >= 1.0e-7 && v.abs() <= 16.0
    };
    let mut regions: Vec<FloatRegion> = Vec::new();
    let mut i = 0;
    while i + 4 <= body.len() {
        if plausible(&body[i..i + 4]) {
            let start = i;
            let mut j = i + 4;
            while j + 4 <= body.len() && plausible(&body[j..j + 4]) {
                j += 4;
            }
            let len = j - start;
            if len >= MIN_REGION_BYTES
                && !bone_tables.iter().any(|t| {
                    let t_end = t.offset + t.bone_count as usize;
                    start < t_end && j > t.offset
                })
            {
                regions.push(FloatRegion {
                    offset: start,
                    byte_count: len,
                });
            }
            i = j;
        } else {
            i += 1;
        }
    }
    // drop regions that start inside another (shifted-phase duplicates)
    let mut kept = Vec::with_capacity(regions.len());
    for (idx, r) in regions.iter().enumerate() {
        let contained = regions.iter().enumerate().any(|(j, other)| {
            idx != j && other.offset <= r.offset && r.offset < other.offset + other.byte_count
        });
        if !contained {
            kept.push(r.clone());
        }
    }
    regions = kept;
    regions
}

/// Pair bone tables with their corresponding float data regions. Each bone
/// table is paired with the next float region that follows it.
// FIT-ANI-07: animation-to-skin join: id join measured, surplus ids dropped
pub fn pair_bone_tables_with_floats(
    body: &[u8],
    bone_tables: &[BoneTable],
    float_regions: &[FloatRegion],
) -> Vec<(usize, u8, usize, Vec<BoneRotation>)> {
    let mut poses = Vec::new();
    for bt in bone_tables {
        for fr in float_regions {
            if fr.offset > bt.offset {
                let float_count = fr.byte_count / 4;
                let bone_count = (bt.bone_count as usize).min(float_count / 3);
                let mut rotations = Vec::with_capacity(bone_count);
                for b in 0..bone_count {
                    let off = fr.offset + b * 12;
                    if off + 12 <= body.len() {
                        let x = f32::from_le_bytes([
                            body[off],
                            body[off + 1],
                            body[off + 2],
                            body[off + 3],
                        ]);
                        let y = f32::from_le_bytes([
                            body[off + 4],
                            body[off + 5],
                            body[off + 6],
                            body[off + 7],
                        ]);
                        let z = f32::from_le_bytes([
                            body[off + 8],
                            body[off + 9],
                            body[off + 10],
                            body[off + 11],
                        ]);
                        rotations.push(BoneRotation { x, y, z });
                    }
                }
                poses.push((bt.offset, bt.bone_count, fr.offset, rotations));
                break;
            }
        }
    }
    poses
}

/// One animated channel: which bones it drives, which frames it keys, and the
/// rotation at each keyed frame.
///
/// Measured on the install (2026-10-16, see `docs/formats/animation.md`
/// § "The keyed-frame sparse lists are confirmed"). The record is:
///
/// ```text
/// [u8 bone indices 1..=N][u8 pad x3][u8 keyed frames, 0-terminated][values]
/// ```
///
/// The keyed-frame list names the frames this channel has a key at (1-based);
/// the values are one **16-byte** (four f32) sample per keyed frame, in the
/// order the keys are listed, living in the clip's shared float block. The
/// sample stores a unit quaternion as `(x, y, z)` in the first three lanes and
/// the fourth lane is not part of the rotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// File offset of the bone-index table (the start of the record).
    pub offset: usize,
    /// The 1-based bone indices this channel drives, in table order.
    pub bones: Vec<u8>,
    /// The 1-based frame indices this channel keys, in stored order.
    pub keys: Vec<u8>,
    /// File offset of the first value sample.
    pub values_offset: usize,
    /// One rotation per keyed frame, in stored order.
    pub rotations: Vec<BoneRotation>,
}

impl Channel {
    /// The rotation at a frame, interpolating between the two surrounding keys
    /// with [`slerp_quat`] (constant angular velocity). Returns `None` outside
    /// the keyed range.
    pub fn rotation_at(&self, frame: f32) -> Option<BoneRotation> {
        if self.keys.is_empty() {
            return None;
        }
        if frame <= *self.keys.first().unwrap() as f32 {
            return self.rotations.first().copied();
        }
        let last = *self.keys.last().unwrap() as f32;
        if frame >= last {
            return self.rotations.last().copied();
        }
        for i in 1..self.keys.len() {
            let a = self.keys[i - 1] as f32;
            let b = self.keys[i] as f32;
            if frame <= b {
                let t = if b > a { (frame - a) / (b - a) } else { 0.0 };
                let (ax, ay, az, aw) = self.rotations[i - 1].quat();
                let (bx, by, bz, bw) = self.rotations[i].quat();
                let q = slerp_quat([ax, ay, az, aw], [bx, by, bz, bw], t);
                return Some(BoneRotation {
                    x: q[0],
                    y: q[1],
                    z: q[2],
                });
            }
        }
        self.rotations.last().copied()
    }
}

/// Range of pad bytes allowed **after the bone list's `0` terminator** and
/// before the keyed-frame list.
///
/// **It is not a constant across families.** Measured on two installed clips:
/// `0x0a0000000000005e` (family `0x006d2000`) has the terminator followed by
/// **two** zero bytes then the key list; `0x0a0000000000c15a` (family
/// `0x00020400`, the first-person family) has **three**. The old code
/// hard-coded three from *before* the terminator, which happened to be right
/// for the first family and wrong for the other, so the reader now skips a
/// short run of zero bytes instead of assuming a fixed gap. A key list cannot
/// start with zero (that is the terminator), so this cannot overshoot into it.
const CHANNEL_KEY_PAD_MAX: usize = 4;

/// How far past the end of the keyed-frame list the value array may start.
///
/// **Bounded by measurement, not by hope.** The values live in the clip's
/// **shared** float block, not next to their record, so the distance is large —
/// coverage over the installed corpus rises monotonically with this window
/// (400 B → 43.7%, 800 B → 50.5%, 2 KB → 55.9%, 8 KB → 65.0%, unbounded →
/// 73.1%), and stays flat past that (the "keys are a subset of a longer sample
/// run" hypothesis moved it only 43.7% → 44.0%, so that is *not* the cause).
/// The default stops at 8 KB: the last 8 points cost an unbounded scan, and a
/// caller that wants them passes a larger window to
/// [`decode_channels_with_window`] explicitly. The search is prefiltered on a
/// unit sample at the candidate start (see [`build_unit_sample_index`]), so the
/// window stays cheap.
pub const CHANNEL_VALUE_WINDOW: usize = 8192;

/// Bytes per value sample. Measured: 16 (four f32), not 12 — see the section
/// reference. This is the value that was wrong in the earlier stride-12 guess.
const CHANNEL_SAMPLE_BYTES: usize = 16;

/// True when `body[off..off+12]` decodes to a unit quaternion stored as
/// `(x, y, z)` with `w = sqrt(1 - x² - y² - z²)`.
///
/// **The all-zero sample is rejected on purpose.** `(0, 0, 0)` gives
/// `w = 1` and is therefore a *valid* unit quaternion (the identity rotation),
/// so a naive test reports a zero-filled region as a long run of quaternions:
/// on the first-person clip that produced **599 false runs**. Real samples in
/// these clips are non-trivial, so a small magnitude floor costs nothing and
/// removes the whole class of false positive.
fn is_unit_sample(body: &[u8], off: usize) -> bool {
    if off + 12 > body.len() {
        return false;
    }
    let mut v = [0f32; 3];
    for (k, slot) in v.iter_mut().enumerate() {
        let b = off + k * 4;
        let f = f32::from_le_bytes([body[b], body[b + 1], body[b + 2], body[b + 3]]);
        if !f.is_finite() || f.abs() > 1.5 {
            return false;
        }
        *slot = f;
    }
    let mag_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if mag_sq > 1.0 {
        return false;
    }
    // reject the all-zero identity, see above
    if mag_sq < 1e-9 {
        return false;
    }
    let w = (1.0 - mag_sq).sqrt();
    let q = (mag_sq + w * w).sqrt();
    (0.98..1.02).contains(&q)
}

/// Every offset in the body that already holds a unit quaternion sample.
///
/// The channel hunt tests each candidate start for a full run of keyed samples;
/// scanning every byte per record repeats that predicate once per record. This
/// builds the candidate set once — one linear pass — so
/// [`decode_channels_with_window`] can binary-search the window instead of
/// re-scanning it. Offsets are byte-exact (the value array's phase is unknown,
/// so no alignment is assumed) and sorted.
pub fn build_unit_sample_index(body: &[u8]) -> Vec<usize> {
    let mut index = Vec::new();
    let mut off = 0usize;
    while off + 12 <= body.len() {
        if is_unit_sample(body, off) {
            index.push(off);
        }
        off += 1;
    }
    index
}

/// Decode every channel in a clip body.
///
/// Uses the default [`CHANNEL_VALUE_WINDOW`]. For a wider (or narrower) hunt,
/// call [`decode_channels_with_window`] directly.
pub fn decode_channels(body: &[u8], frames: u16) -> Vec<Channel> {
    decode_channels_with_window(body, frames, CHANNEL_VALUE_WINDOW)
}

/// Decode every channel, hunting for each value array at most `window` bytes
/// past its keyed-frame list.
///
/// Walks the bone-index tables this module already finds, reads the keyed-frame
/// list that follows each, then locates the value array by requiring *every*
/// keyed frame to decode as a unit quaternion at the 16-byte stride. Records
/// whose values are not found inside the window are skipped rather than
/// guessed — a wrong offset is worse than no record. The candidate starts come
/// from [`build_unit_sample_index`], built once per call.
pub fn decode_channels_with_window(body: &[u8], frames: u16, window: usize) -> Vec<Channel> {
    let index = build_unit_sample_index(body);
    let mut out = Vec::new();
    for bt in find_bone_tables(body) {
        // Skip the zero pad rather than assuming its size (see
        // CHANNEL_KEY_PAD_MAX).
        let mut list = bt.end;
        let pad_limit = (bt.end + CHANNEL_KEY_PAD_MAX).min(body.len());
        while list < pad_limit && body[list] == 0 {
            list += 1;
        }
        if list >= body.len() {
            continue;
        }
        let mut keys = Vec::new();
        while list < body.len() && body[list] != 0 {
            let k = body[list];
            // A key is a frame index; anything at or past `frames` is noise.
            if k == 0 || k as u16 > frames {
                keys.clear();
                break;
            }
            keys.push(k);
            list += 1;
        }
        if keys.len() < 2 || list >= body.len() {
            continue;
        }
        list += 1; // past the terminator
        if keys.len() > frames as usize {
            continue;
        }
        // Locate the value array: at least `keys.len()` consecutive unit
        // samples. `>=` rather than `==` because two shapes are installed —
        // one sample per keyed frame (the character families) and one per
        // frame across the keyed range (the first-person family, measured at
        // 12 keys against 127 samples).
        let need = keys.len() * CHANNEL_SAMPLE_BYTES;
        if need > body.len() {
            continue;
        }
        let hi = list
            .saturating_add(window)
            .min(body.len().saturating_sub(need));
        if hi < list {
            continue;
        }
        let mut found = None;
        // Only offsets that already hold a unit sample can start the array.
        let mut ci = index.partition_point(|&o| o < list);
        while ci < index.len() && index[ci] <= hi {
            let start = index[ci];
            if start + need > body.len() {
                break;
            }
            if (1..keys.len()).all(|i| is_unit_sample(body, start + i * CHANNEL_SAMPLE_BYTES)) {
                found = Some(start);
                break;
            }
            ci += 1;
        }
        let Some(values_offset) = found else { continue };
        // Take every consecutive sample from the array's start, so a dense
        // record contributes one rotation per frame.
        let mut rotations = Vec::new();
        let mut off = values_offset;
        while off + 12 <= body.len() && is_unit_sample(body, off) {
            let mut v = [0f32; 3];
            for (k, slot) in v.iter_mut().enumerate() {
                let b = off + k * 4;
                *slot = f32::from_le_bytes([body[b], body[b + 1], body[b + 2], body[b + 3]]);
            }
            rotations.push(BoneRotation {
                x: v[0],
                y: v[1],
                z: v[2],
            });
            off += CHANNEL_SAMPLE_BYTES;
        }
        if rotations.len() < keys.len() {
            continue;
        }
        out.push(Channel {
            offset: bt.offset,
            bones: bt.bones,
            keys,
            values_offset,
            rotations,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic bytes with the layout the corpus showed: a header, a section
    /// table whose entry 1 is the body start and whose entry 2 opens the second
    /// section, and a body.
    fn synthetic(frames: u16, _records: usize) -> Vec<u8> {
        let second = BODY + 16;
        let mut b = vec![0u8; second + 64];
        b[0x00..0x04].copy_from_slice(&0x006d_2000u32.to_le_bytes());
        b[0x0c..0x0e].copy_from_slice(&frames.to_le_bytes());
        b[0x0e..0x10].copy_from_slice(&30u16.to_le_bytes());
        let dur = frames as f32 / 30.0;
        b[0x14..0x18].copy_from_slice(&dur.to_le_bytes());
        b[0x18..0x1c].copy_from_slice(&(1.0f32 / dur).to_le_bytes());
        b[0x1c..0x20].copy_from_slice(&30.0f32.to_le_bytes());
        b[0x20..0x24].copy_from_slice(&(1.0f32 / 30.0).to_le_bytes());
        b[0x24..0x28].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        b[SECTION_TABLE + 8..SECTION_TABLE + 16].copy_from_slice(&(BODY as i64).to_le_bytes());
        b[SECTION_TABLE + 16..SECTION_TABLE + 24].copy_from_slice(&(second as i64).to_le_bytes());
        b
    }

    #[test]
    fn reads_the_header_the_corpus_showed() {
        let c = Clip::parse(&synthetic(90, 3)).unwrap();
        assert_eq!(c.header.frames, 90);
        assert_eq!(c.header.fps, 30);
        assert_eq!(c.header.family, 0x006d_2000);
        assert!((c.header.duration - 3.0).abs() < 1e-6);
        assert!((c.header.implied_duration() - c.header.duration).abs() < 1e-6);
        // the stored reciprocals agree with the values they invert
        assert!((c.header.rate - c.header.implied_rate()).abs() < 1e-6);
        assert!((c.header.fps_f32 - c.header.fps as f32).abs() < 1e-6);
        assert!((c.header.frame_period - c.header.implied_frame_period()).abs() < 1e-6);
    }

    #[test]
    fn finds_the_sections_and_the_first_section_words() {
        let c = Clip::parse(&synthetic(90, 5)).unwrap();
        assert_eq!(c.second_at, BODY + 16);
        assert_eq!(c.first_section.len(), 4);
        assert!(!c.body.is_empty());
    }

    #[test]
    fn the_evaluators_value_decode_is_the_clients_arithmetic() {
        // 0x8000 is the zero point; each step is 2^-15; a millisecond is added.
        assert!((evaluator_time_value(0x8000) - 0.001).abs() < 1e-9);
        let one_step = evaluator_time_value(0x8001) - evaluator_time_value(0x8000);
        assert!((one_step - 3.051_757_8e-5).abs() < 1e-12);
        // and it is signed: below the zero point the value goes negative
        assert!(evaluator_time_value(0x7fff) < 0.001);
        assert!(evaluator_time_value(0x0000) < -0.99);
    }

    #[test]
    fn a_short_buffer_is_an_error_not_a_panic() {
        assert_eq!(Clip::parse(&[0u8; 4]), Err(ClipError::TooShort));
        assert_eq!(Clip::parse(&[]), Err(ClipError::TooShort));
    }

    #[test]
    fn a_clip_with_no_second_section_is_reported() {
        let mut b = synthetic(30, 0);
        b[SECTION_TABLE + 16..SECTION_TABLE + 24].copy_from_slice(&0i64.to_le_bytes());
        assert_eq!(Clip::parse(&b), Err(ClipError::TableOutOfRange));
    }

    #[test]
    fn bone_tables_are_found() {
        let mut body = vec![0u8; 128];
        body[0] = 1;
        body[1] = 2;
        body[2] = 3;
        body[3] = 4;
        body[4] = 5;
        for i in 0..15 {
            let off = 5 + i * 4;
            if off + 4 <= body.len() {
                body[off..off + 4].copy_from_slice(&(0.5f32).to_le_bytes());
            }
        }
        let tables = find_bone_tables(&body);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].offset, 0);
        assert_eq!(tables[0].bone_count, 5);
    }

    #[test]
    fn float_regions_are_found() {
        let mut body = vec![0u8; 64];
        body[0] = 1;
        body[1] = 2;
        body[2] = 3;
        for i in 0..3 {
            let off = 3 + i * 4;
            if off + 4 <= body.len() {
                body[off..off + 4].copy_from_slice(&(0.5f32).to_le_bytes());
            }
        }
        let regions = find_float_regions(&body);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].offset, 3);
        assert_eq!(regions[0].byte_count, 12);
    }

    #[test]
    fn bone_rotations_are_unit_quaternions() {
        let rot = BoneRotation {
            x: 0.225464,
            y: 0.794954,
            z: -0.110684,
        };
        let (x, y, z, w) = rot.quat();
        let mag = (x * x + y * y + z * z + w * w).sqrt();
        assert!((mag - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pair_bone_tables_with_floats_works() {
        let mut body = vec![0u8; 128];
        body[0] = 1;
        body[1] = 2;
        body[2] = 3;
        for i in 0..9 {
            let off = 3 + i * 4;
            if off + 4 <= body.len() {
                body[off..off + 4].copy_from_slice(&(0.5f32).to_le_bytes());
            }
        }
        let tables = find_bone_tables(&body);
        let regions = find_float_regions(&body);
        let poses = pair_bone_tables_with_floats(&body, &tables, &regions);
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].1, 3);
        assert_eq!(poses[0].3.len(), 3);
    }

    /// A synthetic channel record: 3 bones, pad, a keyed-frame list, then a
    /// 16-byte-per-key value array of unit quaternions.
    fn synthetic_channel() -> (Vec<u8>, usize) {
        let bones = 3usize;
        let keys: Vec<u8> = vec![1, 3, 5];
        let mut body = vec![0u8; 256];
        for i in 0..bones {
            body[i] = (i + 1) as u8;
        }
        // The bone list, its `0` terminator, then `CHANNEL_KEY_PAD` zero bytes
        // before the keyed-frame list. `bt.end` is just past the terminator, so
        // the reader resumes at exactly `bones + 1 + CHANNEL_KEY_PAD`.
        let mut p = bones;
        body[p] = 0; // bone-list terminator
        p += 1;
        p += 2; // CHANNEL_KEY_PAD_MAX allows 2..4; 2 keeps the fixture dense
        for k in &keys {
            body[p] = *k;
            p += 1;
        }
        body[p] = 0; // key-list terminator
        p += 1;
        // Three non-trivial rotations, each a unit quaternion's (x, y, z).
        // The first is deliberately NOT the identity: `(0,0,0)` is a valid unit
        // quaternion but is rejected by `is_unit_sample` on purpose (a
        // zero-filled region otherwise reads as a long quaternion run — 599
        // false positives on the first-person clip), and no installed clip
        // leads a value array with it.
        let samples: [(f32, f32, f32); 3] = [
            (0.0, 0.2, 0.0),
            (0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0),
            (0.6, 0.0, 0.0),
        ];
        for (i, (x, y, z)) in samples.iter().enumerate() {
            let o = p + i * 16;
            body[o..o + 4].copy_from_slice(&x.to_le_bytes());
            body[o + 4..o + 8].copy_from_slice(&y.to_le_bytes());
            body[o + 8..o + 12].copy_from_slice(&z.to_le_bytes());
            body[o + 12..o + 16].copy_from_slice(&0f32.to_le_bytes());
        }
        (body, p)
    }

    #[test]
    fn decode_channels_reads_bones_keys_and_values() {
        let (body, values_off) = synthetic_channel();
        let chans = decode_channels(&body, 21);
        assert_eq!(chans.len(), 1, "one channel expected");
        let c = &chans[0];
        assert_eq!(c.bones, vec![1, 2, 3]);
        assert_eq!(c.keys, vec![1, 3, 5]);
        assert_eq!(c.values_offset, values_off);
        assert_eq!(c.rotations.len(), 3);
        // every decoded rotation must be a unit quaternion
        for r in &c.rotations {
            let (x, y, z, w) = r.quat();
            let m = (x * x + y * y + z * z + w * w).sqrt();
            assert!((m - 1.0).abs() < 1e-4, "not unit: {m}");
        }
    }

    #[test]
    fn channel_value_stride_is_16_not_12() {
        // The stride is pinned by requiring the *decoded values* to equal what
        // was written. At a 12-byte stride the samples would overlap and the
        // recovered rotations would differ from these.
        let (body, _) = synthetic_channel();
        let c = &decode_channels(&body, 21)[0];
        let expected: [(f32, f32, f32); 3] = [
            (0.0, 0.2, 0.0),
            (0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0),
            (0.6, 0.0, 0.0),
        ];
        assert_eq!(c.rotations.len(), expected.len());
        for (got, (x, y, z)) in c.rotations.iter().zip(expected) {
            assert!((got.x - x).abs() < 1e-6, "x: {} vs {x}", got.x);
            assert!((got.y - y).abs() < 1e-6, "y: {} vs {y}", got.y);
            assert!((got.z - z).abs() < 1e-6, "z: {} vs {z}", got.z);
        }
    }

    #[test]
    fn rotation_at_interpolates_between_keys() {
        let (body, _) = synthetic_channel();
        let c = &decode_channels(&body, 21)[0];
        // exactly on a key
        let on_key = c.rotation_at(3.0).unwrap();
        assert!((on_key.y - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        // halfway between key 1 (y=0.2) and key 3 (y=0.7071): slerp, so the
        // midpoint is unit-length and lies strictly between the endpoints
        // (never asserted equal to the linear blend — see `slerp_quat`).
        let mid = c.rotation_at(2.0).unwrap();
        let (x, y, z, w) = mid.quat();
        assert!((x * x + y * y + z * z + w * w - 1.0).abs() < 1e-5);
        assert!(
            mid.y > 0.2 && mid.y < std::f32::consts::FRAC_1_SQRT_2,
            "mid.y = {}",
            mid.y
        );
        // clamped outside the keyed range
        assert!((c.rotation_at(0.0).unwrap().y - 0.2).abs() < 1e-6);
        assert!((c.rotation_at(99.0).unwrap().x - 0.6).abs() < 1e-5);
    }

    #[test]
    fn decode_channels_skips_records_without_values() {
        // A record whose value array is absent must be skipped, not guessed.
        let (body, _) = synthetic_channel();
        let truncated = &body[..body.len() - 200];
        assert!(decode_channels(truncated, 21).is_empty());
    }
}

/// A track kind, mirroring the reference implementation's
/// `TrackType { Position = 0, Rotation = 1, Scale = 2 }`
/// (`E:/CASCReader/OWLib/TankLib/teAnimation.cs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Position,
    Rotation,
    Scale,
}

/// One bone's rotation track: a unit quaternion at each keyed frame.
///
/// **Storage is three packed u16, not three f32.** Verified against the
/// installed clip `0x0a0000000000005e`: decoding with the reference's
/// arithmetic — the top bits of the first two words carry an *axis order*, the
/// remaining 15 bits are scaled by `√2` so three components carry half the
/// quaternion — every keyed frame comes out with `|q| = 1.000000`. The fourth
/// component is `√(1 − min(x²+y²+z², 1))` and the four are then *reordered* by
/// the axis.
///
/// This also explains the earlier "16-byte stride, unit quaternions" reading:
/// that was an alignment coincidence over an adjacent block. The true stride
/// is **6 bytes per keyed frame**.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationTrack {
    pub offset: usize,
    pub track: TrackKind,
    /// The keyed frames, in stored order (1-based).
    pub keys: Vec<u16>,
    /// One quaternion per key, in stored order, `(x, y, z, w)`.
    pub quats: Vec<[f32; 4]>,
}

/// Decode one packed legacy rotation at `bytes[off..off+6]`.
///
/// Ported from `teAnimation.ReadRotation` / `UnpackOldRotation` /
/// `QuatFromAxisOrder`. Returns `None` when the three components already exceed
/// unit length, which is how a misaligned read is rejected.
pub fn decode_packed_rotation(bytes: &[u8], off: usize) -> Option<[f32; 4]> {
    if off + 6 > bytes.len() {
        return None;
    }
    let a = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
    let b = u16::from_le_bytes([bytes[off + 2], bytes[off + 3]]);
    let c = u16::from_le_bytes([bytes[off + 4], bytes[off + 5]]);
    // top bits carry the axis order
    let axis = ((b >> 15) << 1) | (a >> 15);
    let a = a & 0x7FFF;
    let b = b & 0x7FFF;
    const SQRT2: f32 = std::f32::consts::SQRT_2;
    const HALF_BITS: f32 = 0x4000 as f32;
    const FULL_BITS: f32 = 0x8000 as f32;
    let (af, bf, cf) = (a as f32, b as f32, c as f32);
    let x = SQRT2 * (af - HALF_BITS) / FULL_BITS;
    let y = SQRT2 * (bf - HALF_BITS) / FULL_BITS;
    let z = (cf - FULL_BITS) / (FULL_BITS * 2.0);
    let s = x * x + y * y + z * z;
    if s > 1.0 {
        return None;
    }
    let w = (1.0 - s).sqrt();
    Some(match axis {
        0 => [w, x, y, z],
        1 => [x, w, y, z],
        2 => [x, y, w, z],
        _ => [x, y, z, w],
    })
}

/// Find every bone's rotation track in a clip body.
///
/// The record shape, measured: a `0`-terminated ascending bone list, a short run
/// of zero pad, a `0`-terminated list of **keyed frames**, then the values. The
/// values do **not** start at a fixed distance — the per-record gap measured on
/// clip `0x0a0000000000005e` is 4, 9, 6, 6 and 29 bytes for five records,
/// because each record also carries position and scale tracks of differing
/// lengths. So the value offset is *searched*, requiring **every** keyed frame to
/// decode as a unit quaternion at the 6-byte stride.
///
/// Returns one track per record that decodes cleanly.
pub fn decode_rotation_tracks(body: &[u8], frames: u16) -> Vec<RotationTrack> {
    let info_size = frames as usize + 1;
    let mut out = Vec::new();
    for bt in find_bone_tables(body) {
        // key list: skip zero pad, then read the 0-terminated frame list
        let mut list = bt.end;
        let limit = (bt.end + 4).min(body.len());
        while list < limit && body[list] == 0 {
            list += 1;
        }
        let mut keys: Vec<u16> = Vec::new();
        while list < body.len() && body[list] != 0 {
            keys.push(body[list] as u16);
            list += 1;
        }
        let terminator = list;
        if keys.len() < 2 || terminator >= body.len() {
            continue;
        }
        // The reference computes the frame as abs(index) % InfoTableSize.
        let keys: Vec<u16> = keys
            .iter()
            .map(|k| (*k as usize % info_size) as u16)
            .collect();

        // Search for the value array: the gap is per-record, not constant.
        let mut best: Option<(usize, Vec<[f32; 4]>)> = None;
        for gap in 1..64usize {
            let base = terminator + gap;
            if base + 6 * keys.len() > body.len() {
                break;
            }
            let mut qs = Vec::with_capacity(keys.len());
            let mut ok = true;
            for i in 0..keys.len() {
                match decode_packed_rotation(body, base + i * 6) {
                    Some(q) => qs.push(q),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                best = Some((base, qs));
                break;
            }
        }
        if let Some((offset, quats)) = best {
            out.push(RotationTrack {
                offset,
                track: TrackKind::Rotation,
                keys,
                quats,
            });
        }
    }
    out
}

/// A bone's position or scale track: three components per keyed frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Vec3Track {
    pub track: TrackKind,
    pub keys: Vec<u16>,
    /// One `[x, y, z]` per key, in stored order.
    pub values: Vec<[f32; 3]>,
}

/// Decode a packed scale sample: three `u16` divided by 1024.
///
/// Ported from `teAnimation.ReadScale` / `UnpackScale`.
pub fn decode_packed_scale(bytes: &[u8], off: usize) -> [f32; 3] {
    let w = |i: usize| {
        if off + i * 2 + 2 > bytes.len() {
            0.0
        } else {
            u16::from_le_bytes([bytes[off + i * 2], bytes[off + i * 2 + 1]]) as f32 / 1024.0
        }
    };
    [w(0), w(1), w(2)]
}

/// Decode an IEEE-754 binary16 bit pattern into `f32`.
///
/// `f16` is still unstable on this toolchain, so the conversion is written out
/// (it is the standard layout: 1 sign, 5 exponent, 10 mantissa, bias 15).
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0f32 } else { 1.0f32 };
    let exp = ((bits >> 10) & 0x1F) as i32;
    let mant = (bits & 0x03FF) as f32;
    if exp == 0 {
        // subnormal or zero
        sign * mant * (1.0 / 16777216.0)
    } else if exp == 0x1F {
        // inf / nan
        if mant == 0.0 {
            sign * f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        let e = (exp - 15) as f32;
        sign * (1.0 + mant / 1024.0) * 2f32.powf(e)
    }
}

/// Decode a packed position sample.
///
/// Ported from `teAnimation.ReadPosition`: the new format is three **f16**
/// divided by 32 in 10 bytes; the legacy format is three f32 in 12 bytes,
/// selected by the caller's `legacy` flag.
pub fn decode_packed_position(bytes: &[u8], off: usize, legacy: bool) -> [f32; 3] {
    if legacy {
        let mut v = [0f32; 3];
        for (i, slot) in v.iter_mut().enumerate() {
            let o = off + i * 4;
            *slot = if o + 4 <= bytes.len() {
                f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
            } else {
                0.0
            };
        }
        v
    } else {
        let mut v = [0f32; 3];
        for (i, slot) in v.iter_mut().enumerate() {
            let o = off + i * 2;
            *slot = if o + 2 <= bytes.len() {
                let bits = u16::from_le_bytes([bytes[o], bytes[o + 1]]);
                f16_to_f32(bits) / 32.0
            } else {
                0.0
            };
        }
        v
    }
}

/// Decode the **new** packed rotation at `bytes[off..off+8]`: four `u16`.
///
/// Ported from `teAnimation.UnpackNewRotation`. The fourth word `d` carries
/// three extra mantissa fragments plus a 2-bit axis order:
///
/// ```text
/// a' = a << 5  | (d      & 0x1F)     21 bits, centred at 0x100000
/// b' = b << 4  | ((d >> 5)  & 0xF)   19 bits, centred at 0x80000
/// c' = c << 5  | ((d >> 9)  & 0x1F) 21 bits, centred at 0x100000
/// axis = d >> 14
/// ```
///
/// and the same `√2`-halving and axis reorder as the legacy form, with an extra
/// factor of ½ because the mantissas are twice as wide.
pub fn decode_packed_rotation_new(bytes: &[u8], off: usize) -> Option<[f32; 4]> {
    if off + 8 > bytes.len() {
        return None;
    }
    let w = |i: usize| u16::from_le_bytes([bytes[off + i * 2], bytes[off + i * 2 + 1]]);
    let (a, b, c, d) = (w(0), w(1), w(2), w(3));

    let a_pre = ((a as u32) << 5) | ((d as u32) & 0x1F);
    let b_pre = ((b as u32) << 4) | (((d as u32) >> 5) & 0xF);
    let c_pre = ((c as u32) << 5) | (((d as u32) >> 9) & 0x1F);
    let axis = d >> 14;

    const SQRT2: f32 = std::f32::consts::SQRT_2;
    const HALF: f32 = 0.5;
    const CENTRE_21: f32 = 0x10_0000 as f32;
    const SPAN_21: f32 = 0x10_0000 as f32;
    const CENTRE_19: f32 = 0x8_0000 as f32;
    const SPAN_19: f32 = 0x8_0000 as f32;
    let x = HALF * SQRT2 * (a_pre as f32 - CENTRE_21) / SPAN_21;
    let y = HALF * SQRT2 * (b_pre as f32 - CENTRE_19) / SPAN_19;
    let z = HALF * SQRT2 * (c_pre as f32 - CENTRE_21) / SPAN_21;
    let s = x * x + y * y + z * z;
    if s > 1.0 {
        return None;
    }
    let wq = (1.0 - s).sqrt();
    Some(match axis {
        0 => [wq, x, y, z],
        1 => [x, wq, y, z],
        2 => [x, y, wq, z],
        _ => [x, y, z, wq],
    })
}

/// Bytes per sample for a track kind, for the *search* that locates a value
/// array. The new position format is 10 bytes on the wire but only 6 of those
/// carry the components (three f16 plus a packed u32 the reference does not
/// decode), so a search tries both.
fn sample_step(kind: TrackKind, legacy_position: bool) -> usize {
    match kind {
        TrackKind::Rotation => 6,
        TrackKind::Scale => 6,
        TrackKind::Position => {
            if legacy_position {
                12
            } else {
                10
            }
        }
    }
}

/// Find every bone's position and scale tracks in a clip body.
///
/// Uses the same record walk as [`decode_rotation_tracks`]: bone list, zero
/// pad, key list, then a **searched** gap before the values. The three track
/// kinds share the record, so one walk collects all of them.
pub fn decode_vec_tracks(body: &[u8], _frames: u16) -> Vec<Vec3Track> {
    let mut out = Vec::new();
    for bt in find_bone_tables(body) {
        let mut list = bt.end;
        let limit = (bt.end + 4).min(body.len());
        while list < limit && body[list] == 0 {
            list += 1;
        }
        let mut keys: Vec<u16> = Vec::new();
        while list < body.len() && body[list] != 0 {
            keys.push(body[list] as u16);
            list += 1;
        }
        let terminator = list;
        if keys.len() < 2 || terminator >= body.len() {
            continue;
        }
        // scale: three u16 per key
        if let Some((off, _)) =
            find_vec_array(body, terminator, keys.len(), TrackKind::Scale, false)
        {
            let values = (0..keys.len())
                .map(|i| decode_packed_scale(body, off + i * 6))
                .collect();
            out.push(Vec3Track {
                track: TrackKind::Scale,
                keys: keys.clone(),
                values,
            });
        }
        // position: try both formats
        for legacy in [false, true] {
            if let Some((off, _)) =
                find_vec_array(body, terminator, keys.len(), TrackKind::Position, legacy)
            {
                let step = sample_step(TrackKind::Position, legacy);
                let values = (0..keys.len())
                    .map(|i| decode_packed_position(body, off + i * step, legacy))
                    .collect();
                out.push(Vec3Track {
                    track: TrackKind::Position,
                    keys: keys.clone(),
                    values,
                });
                break;
            }
        }
    }
    out
}

/// Locate a position/scale value array: the first offset past the key list where
/// **every** keyed frame decodes to a plausible value.
///
/// Plausibility is deliberately loose (finite, bounded) because these channels
/// have no unit-length invariant to test against the way a quaternion does.
fn find_vec_array(
    body: &[u8],
    terminator: usize,
    nkeys: usize,
    kind: TrackKind,
    legacy_position: bool,
) -> Option<(usize, u32)> {
    let step = sample_step(kind, legacy_position);
    for gap in 1..256usize {
        let base = terminator + gap;
        if base + step * nkeys > body.len() {
            break;
        }
        let mut ok = true;
        for i in 0..nkeys {
            let o = base + i * step;
            let v = match kind {
                TrackKind::Scale => decode_packed_scale(body, o),
                _ => decode_packed_position(body, o, legacy_position),
            };
            for c in v {
                if !c.is_finite() || c.abs() > 4096.0 {
                    ok = false;
                    break;
                }
            }
            if !ok {
                break;
            }
        }
        if ok {
            return Some((base, 0));
        }
    }
    None
}

/// Find every bone's rotation track using the **new** (8-byte) packing.
///
/// Same record walk as [`decode_rotation_tracks`], but the value array is
/// searched at the 8-byte stride instead of 6, so a caller can tell which
/// packing a clip uses and report both.
pub fn decode_rotation_tracks_new(body: &[u8], frames: u16) -> Vec<RotationTrack> {
    let mut out = Vec::new();
    for bt in find_bone_tables(body) {
        let mut list = bt.end;
        let limit = (bt.end + 4).min(body.len());
        while list < limit && body[list] == 0 {
            list += 1;
        }
        let mut keys: Vec<u16> = Vec::new();
        while list < body.len() && body[list] != 0 {
            keys.push(body[list] as u16);
            list += 1;
        }
        let terminator = list;
        if keys.len() < 2 || terminator >= body.len() {
            continue;
        }
        let info_size = frames as usize + 1;
        let keys: Vec<u16> = keys
            .iter()
            .map(|k| (*k as usize % info_size) as u16)
            .collect();
        let mut found = None;
        for gap in 1..96usize {
            let base = terminator + gap;
            if base + 8 * keys.len() > body.len() {
                break;
            }
            let mut qs = Vec::with_capacity(keys.len());
            let mut ok = true;
            for i in 0..keys.len() {
                match decode_packed_rotation_new(body, base + i * 8) {
                    Some(q) => qs.push(q),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                found = Some((base, qs));
                break;
            }
        }
        if let Some((offset, quats)) = found {
            out.push(RotationTrack {
                offset,
                track: TrackKind::Rotation,
                keys,
                quats,
            });
        }
    }
    out
}

/// `InfoTableFlags`, mirroring the reference implementation's enum
/// (`E:/CASCReader/OWLib/TankLib/teAnimation.cs`). Only the two bits that
/// select a **value format** are named; the rest are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackFlags(pub u16);

impl TrackFlags {
    /// `LegacyPositionFormat = 0x200` — when **set**, position is three f32
    /// (12 bytes); when clear, three f16 ÷ 32 (10 bytes).
    pub const LEGACY_POSITION_FORMAT: u16 = 0x200;
    /// `NewRotationCompression = 0x400` — when set, rotation is the 8-byte
    /// bit-packed form rather than the 6-byte one.
    pub const NEW_ROTATION_COMPRESSION: u16 = 0x400;

    pub fn legacy_position(&self) -> bool {
        self.0 & Self::LEGACY_POSITION_FORMAT != 0
    }
    pub fn new_rotation(&self) -> bool {
        self.0 & Self::NEW_ROTATION_COMPRESSION != 0
    }
}

/// What a record's rotation track actually used, and whether the choice came
/// from the format's own flag or from the fallback search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationSource {
    /// The `0x400` flag said so.
    FlagNew,
    /// The flag was clear and the 6-byte form decoded.
    FlagLegacy,
    /// No usable flag; the search found this packing.
    SearchFallback,
}

/// One decoded rotation track with its provenance.
#[derive(Debug, Clone)]
pub struct SourcedRotationTrack {
    pub track: RotationTrack,
    pub source: RotationSource,
}

/// One bone's complete animation data: which bone it is, and its rotation track.
///
/// **The pairing is positional** — the *i*-th track belongs to the *i*-th bone
/// list, which is how the reference implementation pairs them (a plain loop
/// counter over `BoneCount`; there is no lookup table — the ids in `BoneList`
/// are labels, not a join key).
#[derive(Debug, Clone)]
pub struct BoneTrack {
    /// The bones this track applies to, from the record's bone list.
    pub bones: Vec<u8>,
    /// The keyed frames (1-based) and the rotation at each.
    pub keys: Vec<u16>,
    pub quats: Vec<[f32; 4]>,
    /// File offset of the value array, for diagnostics.
    pub values_offset: usize,
}

impl BoneTrack {
    /// The rotation at `frame`, swept between the surrounding keys with
    /// [`slerp_quat`]. Returns the first or last key's value when `frame` is
    /// outside the keyed range.
    pub fn quat_at(&self, frame: f32) -> [f32; 4] {
        if self.keys.is_empty() {
            return [0.0, 0.0, 0.0, 1.0];
        }
        if frame <= self.keys[0] as f32 {
            return self.quats[0];
        }
        if frame >= *self.keys.last().unwrap() as f32 {
            return *self.quats.last().unwrap();
        }
        for i in 1..self.keys.len() {
            let a = self.keys[i - 1] as f32;
            let b = self.keys[i] as f32;
            if frame <= b {
                let t = if b > a { (frame - a) / (b - a) } else { 0.0 };
                return slerp_quat(self.quats[i - 1], self.quats[i], t);
            }
        }
        *self.quats.last().unwrap()
    }
}

/// The total angular travel of a candidate value array, in degrees.
///
/// **Why this exists.** `is_unit_sample`/`decode_packed_rotation` are nearly free
/// to satisfy — the quaternion's fourth component is *derived*, so any six bytes
/// whose three components are small enough decode to a valid-looking rotation.
/// Measured on the installed clip `0x0a0000000000005e`: **60.3% of randomly
/// chosen offsets pass**. A search that takes the first passing offset therefore
/// takes noise, which is exactly what happened — every track it found moved
/// 150–178° between adjacent frames.
///
/// A real animation channel is *smooth*: adjacent keyed frames differ by a small
/// angle. Scoring candidates by total angular travel turns a weak predicate into
/// a strong one, and the true array is the minimum.
fn smoothness_score(quats: &[[f32; 4]]) -> f32 {
    let mut total = 0.0f32;
    for w in quats.windows(2) {
        let (p, q) = (w[0], w[1]);
        let dot = (p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3]).abs();
        total += 2.0 * dot.clamp(-1.0, 1.0).acos().to_degrees();
    }
    total
}

/// How far past the key list to look for the value array. The gap is per-record
/// (position and scale tracks sit between), so this is a window, not a stride.
const VALUE_SEARCH_WINDOW: usize = 512;

/// Read every bone's rotation track, pairing each with its bone list.
///
/// One walk per record, trying the **legacy 6-byte** packing first and the
/// **new 8-byte** one second, so a clip that uses either decodes. Returns the
/// tracks in file order.
pub fn decode_bone_tracks(body: &[u8], frames: u16) -> Vec<BoneTrack> {
    let info_size = frames as usize + 1;
    let mut out = Vec::new();
    for bt in find_bone_tables(body) {
        // key list: skip the zero pad, then read the 0-terminated frame list
        let mut list = bt.end;
        let limit = (bt.end + 4).min(body.len());
        while list < limit && body[list] == 0 {
            list += 1;
        }
        let mut keys: Vec<u16> = Vec::new();
        while list < body.len() && body[list] != 0 {
            keys.push(body[list] as u16);
            list += 1;
        }
        let terminator = list;
        if keys.len() < 2 || terminator >= body.len() {
            continue;
        }
        let keys: Vec<u16> = keys
            .iter()
            .map(|k| (*k as usize % info_size) as u16)
            .collect();

        // Score every candidate offset for **both** packings and keep the
        // smoothest. See `smoothness_score`: the unit-quaternion test alone is
        // satisfied by 60% of random offsets, so smoothness is what actually
        // identifies the array.
        let mut best: Option<(f32, usize, Vec<[f32; 4]>)> = None;
        for &stride in &[6usize, 8usize] {
            let decode: fn(&[u8], usize) -> Option<[f32; 4]> = if stride == 6 {
                decode_packed_rotation
            } else {
                decode_packed_rotation_new
            };
            for gap in 1..VALUE_SEARCH_WINDOW {
                let base = terminator + gap;
                if base + stride * keys.len() > body.len() {
                    break;
                }
                let mut qs = Vec::with_capacity(keys.len());
                let mut ok = true;
                for i in 0..keys.len() {
                    match decode(body, base + i * stride) {
                        Some(q) => qs.push(q),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    continue;
                }
                let score = smoothness_score(&qs);
                if best.as_ref().map(|(b, _, _)| score < *b).unwrap_or(true) {
                    best = Some((score, base, qs));
                }
            }
        }
        let found = best.map(|(_, off, qs)| (off, qs));
        if let Some((values_offset, quats)) = found {
            out.push(BoneTrack {
                bones: bt.bones.clone(),
                keys,
                quats,
                values_offset,
            });
        }
    }
    out
}

/// SSE `MINSS`: `a` when `a < b`, otherwise `b`.
///
/// `<` is false on NaN, so a NaN first source falls through to the second
/// source. That is the client's `minss`, not `f32::min` (which would return
/// the non-NaN operand).
fn minss(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

/// SSE `MAXSS`: `a` when `a > b`, otherwise `b`. NaN falls through to the
/// second source, as in [`minss`].
fn maxss(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

/// x86 `CVTTSS2SI r32, xmm`: truncate toward zero, returning the integer
/// indeterminate `0x80000000` for NaN, ±infinity and anything outside `i32`.
/// Rust's `as i32` saturates instead, which is a different function.
fn cvttss2si(x: f32) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// The client's animation-time wrap, `AnimTimeWrap` (`0x140b91ef0`).
///
/// `range` is the animation node's `float[4]`: `[0]` is the minimum, `[1]` the
/// maximum (and the period when wrapping), `[2]` is not read by the wrap, and
/// `[3]`'s bits are the node's flag dword. **Bit 0** of that dword selects the
/// truncating modular wrap; with the bit clear the value is clamped with SSE
/// `minss`/`maxss`, so a NaN input falls through to the bound rather than
/// propagating.
///
/// The truncating wrap is `t - (float)(int)(t / max) * max`, *not* `t.fract()`
/// or `rem_euclid`: for a negative `t` the truncating form stays negative
/// (`-1.5` over a period of `2` gives `-1.5`), where a Euclidean remainder
/// would give `+0.5`. That difference is what the harness fixture's
/// flag-bit-sweep cases pin.
///
/// Verified as `fn/AnimTimeWrap` and its tier-2 Rust twin
/// `fn/AnimTimeWrap-rust`; the tests below are that unit's own discriminating
/// cases.
///
/// The range is the animation **node record's** own first 16 bytes, which the
/// sampler passes straight to the wrap (`0x140b9a790`: the node pointer is
/// loaded and `AnimTimeWrap(node, t)` is called on it). It is runtime state,
/// not a clip-header field, so [`PoseClip::frame_at`] can only supply the
/// clip's own range and the clear-flag branch.
pub fn anim_time_wrap(range: [f32; 4], t: f32) -> f32 {
    let min = range[0];
    let max = range[1];
    let flag = range[3].to_bits();
    if flag & 1 != 0 {
        let n = cvttss2si(t / max);
        t - (n as f32) * max
    } else {
        maxss(minss(t, max), min)
    }
}

/// A decoded clip ready to pose a rig with.
///
/// **The bone pairing is positional**, matching the reference implementation:
/// the *i*-th record's track drives bone *i* (`BoneAnimations[i]` pairs with
/// `BoneList[i]` through a loop counter — there is no lookup table, the names
/// in the bone list are labels). A bone with no track stays in bind pose.
#[derive(Debug, Clone)]
pub struct PoseClip {
    /// `tracks[bone]` is that bone's rotation over time, or `None` where the
    /// clip does not animate it.
    pub tracks: Vec<Option<BoneTrack>>,
    pub frames: u16,
    pub fps: f32,
}

impl PoseClip {
    /// Decode a clip into a pose source.
    pub fn decode(body: &[u8], frames: u16, fps: f32) -> PoseClip {
        let decoded = decode_bone_tracks(body, frames);
        let mut tracks: Vec<Option<BoneTrack>> = Vec::new();
        for (i, t) in decoded.into_iter().enumerate() {
            if tracks.len() <= i {
                tracks.resize(i + 1, None);
            }
            tracks[i] = Some(t);
        }
        PoseClip {
            tracks,
            frames,
            fps,
        }
    }

    /// How many bones the clip actually animates.
    pub fn animated_bones(&self) -> usize {
        self.tracks.iter().filter(|t| t.is_some()).count()
    }

    /// The rotation for `bone` at `frame`, or `None` to leave it in bind pose.
    pub fn quat_at(&self, bone: usize, frame: f32) -> Option<[f32; 4]> {
        self.tracks
            .get(bone)
            .and_then(|t| t.as_ref())
            .map(|t| t.quat_at(frame))
    }

    /// The frame number at a time in seconds, wrapped into the clip's range.
    ///
    /// This is the client's `AnimTimeWrap` (`0x140b91ef0`) over the clip's own
    /// range `[0, frames]` with the node's flag dword **clear** — the read's
    /// clamp branch. The wrap branch needs the animation node's flag (bit 0 of
    /// the dword at node `+0x0c`), and the node record is runtime state the
    /// clip header does not carry, so it is not invented here; a caller that
    /// has a read range calls [`anim_time_wrap`] directly.
    ///
    /// There is no `+1`: the stored keys are `abs(index) % (frames + 1)`, so
    /// they are 0-based and frame 0 is a real key.
    pub fn frame_at(&self, seconds: f32) -> f32 {
        let n = self.frames.max(1) as f32;
        anim_time_wrap([0.0, n, 0.0, 0.0], seconds * self.fps)
    }
}

/// Where a record kind's handler lives.
///
/// *Decompiled*: the dispatcher at v2b `0x7ff7c58df660` (live
/// `0x7ff613ecf660` = v2b − `0x1B1A10000` in the practice-range session)
/// tail-jumps through a 31-entry table; the entries below are that table's
/// targets. A page that is zero-filled in the flattened v2b image
/// (`module_v2full.bin`, base `0x7ff7c4d20000`) and absent from `dumps_v2`,
/// `dumps_v2b` and `dumps_range` alike has never been observed executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindResidency {
    /// Handler is inline in the dispatcher page (readable in all sessions).
    InlineHandler,
    /// Handler body is present in at least the v2b capture.
    ResidentHandler,
    /// Handler address is known but its page is zero-filled in every standing
    /// capture — never observed, never executed here.
    ColdUnverified,
    /// Falls through to the `XOR AL, AL` stub: the kind decodes to false.
    DefaultStub,
}

/// One entry of the record-kind dispatch: what kind `n` (1-based) does.
///
/// Kind 0 underflows the dispatcher's `kind − 1` index and lands on the default
/// stub, like any kind above 31.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindInfo {
    /// The 1-based kind byte value at `record + 0xe`.
    pub kind: u8,
    /// Where the handler lives, if anywhere.
    pub residency: KindResidency,
    /// The handler's v2b address, or the default stub's for stub kinds.
    pub handler_v2b: Option<u64>,
    /// One line on why this classification holds.
    pub note: &'static str,
}

/// The default stub every unhandled kind lands on (v2b).
///
/// *Decompiled*: `FUN_7ff7c58df913` — `XOR AL, AL`, i.e. the kind evaluates to
/// false/absent.
pub const KIND_DEFAULT_STUB_V2B: u64 = 0x7ff7_c58d_f913;

/// The full kind → handler map, 31 entries.
///
/// *Decompiled* (jump table live `0x7ff613ecf928` = v2b `0x7ff7c58df928`, entry
/// `kind − 1`; cross-checked against `D:/ow-dumps/handlers.txt`, which fixed an
/// off-by-one in the working labels). Residency is *capture-data*: which pages
/// are non-zero in the flattened v2b image and the `dumps_v2` / `dumps_v2b` /
/// `dumps_range` captures.
pub static KIND_TABLE: [KindInfo; 31] = [
    KindInfo {
        kind: 1,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58e_d7b0),
        note: "PAGE_NOACCESS in the 10-15 live poll; zero-filled since",
    },
    KindInfo {
        kind: 2,
        residency: KindResidency::ResidentHandler,
        handler_v2b: Some(0x7ff7_c58e_a110),
        note: "the record evaluator; v2b page base 0x7ff7c58ea000 is non-zero",
    },
    KindInfo {
        kind: 3,
        residency: KindResidency::InlineHandler,
        handler_v2b: Some(0x7ff7_c58d_f6cd),
        note: "inline in the dispatcher page, readable in all sessions",
    },
    KindInfo {
        kind: 4,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58f_2ee0),
        note: "zero-filled in the flattened v2b image",
    },
    KindInfo {
        kind: 5,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58e_c1f0),
        note: "zero-filled in the flattened v2b image",
    },
    KindInfo {
        kind: 6,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 7,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58e_6930),
        note: "zero-filled in the flattened v2b image",
    },
    KindInfo {
        kind: 8,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58e_ebd0),
        note: "zero-filled in the flattened v2b image",
    },
    KindInfo {
        kind: 9,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 10,
        residency: KindResidency::ResidentHandler,
        handler_v2b: Some(0x7ff7_c58e_bbf0),
        note: "double-indirect via tables at RVA 0xBCC18C/0xBCC138, whose page is cold",
    },
    KindInfo {
        kind: 11,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 12,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 13,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 14,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 15,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58f_5620),
        note: "zero-filled in the flattened v2b image",
    },
    KindInfo {
        kind: 16,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 17,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 18,
        residency: KindResidency::InlineHandler,
        handler_v2b: Some(0x7ff7_c58d_f854),
        note: "inline in the dispatcher page, readable in all sessions",
    },
    KindInfo {
        kind: 19,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58f_6a50),
        note: "zero-filled in the flattened v2b image",
    },
    KindInfo {
        kind: 20,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 21,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 22,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 23,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 24,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 25,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 26,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 27,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 28,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 29,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 30,
        residency: KindResidency::DefaultStub,
        handler_v2b: Some(KIND_DEFAULT_STUB_V2B),
        note: "shares the default stub",
    },
    KindInfo {
        kind: 31,
        residency: KindResidency::ColdUnverified,
        handler_v2b: Some(0x7ff7_c58e_7f50),
        note: "zero-filled in the flattened v2b image",
    },
];

/// Look up a kind byte in the dispatch table.
///
/// Returns `None` for kind 0 and anything above 31 — both land on the default
/// stub in the client, which this port reports as "no entry" rather than
/// executing.
pub fn kind_info(kind: u8) -> Option<&'static KindInfo> {
    if (1..=31).contains(&kind) {
        Some(&KIND_TABLE[(kind - 1) as usize])
    } else {
        None
    }
}

/// Read the kind byte of the record at `record_at`.
///
/// Returns `None` when the record would run past the buffer. The caller must
/// have gated the stride with [`record_stride_for_family`]: this reads one
/// byte and knows nothing about families.
pub fn read_record_kind(bytes: &[u8], record_at: usize) -> Option<u8> {
    bytes.get(record_at + RECORD_KIND_OFFSET).copied()
}

/// Spherical-linear interpolation between two unit quaternions.
///
/// *Published-check*: this is the standard slerp — constant angular velocity
/// along the great arc — with the double-cover sign flip (`dot < 0` negates
/// one operand so the short arc is taken) and a lerp-plus-renormalise fallback
/// past `dot > 0.9995`, where the arc is degenerate. No game bytes involved;
/// the justification is geometric, and it is what [`Channel::rotation_at`] and
/// [`BoneTrack::quat_at`] use between keys.
pub fn slerp_quat(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut b = b;
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        dot = -dot;
        b = [-b[0], -b[1], -b[2], -b[3]];
    }
    if dot > 0.9995 {
        let mut o = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ];
        let m = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2] + o[3] * o[3]).sqrt();
        if m > 1e-9 {
            o[0] /= m;
            o[1] /= m;
            o[2] /= m;
            o[3] /= m;
        } else {
            o = a;
        }
        return o;
    }
    let theta = dot.clamp(-1.0, 1.0).acos();
    let sin = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / sin;
    let wb = (t * theta).sin() / sin;
    [
        wa * a[0] + wb * b[0],
        wa * a[1] + wb * b[1],
        wa * a[2] + wb * b[2],
        wa * a[3] + wb * b[3],
    ]
}

/// The frame-count modulus the reference uses for frame indices.
///
/// *Reference* (`teAnimation.cs`, `InfoTableSize = (int)(FPS * Duration) + 1`),
/// *verified* to agree with [`Header::frames`] on three installed clips
/// (21, 170, 5). The decoders take `frames` and add one, which is this.
pub fn frame_key_modulus(frames: u16) -> usize {
    frames as usize + 1
}

/// The stored width of one frame index for a clip of this size.
///
/// *Reference* (`teAnimation.cs`, `ReadIndices`): one byte while
/// `InfoTableSize ≤ 255`, else one little-endian `i16`.
pub fn frame_index_width(info_table_size: usize) -> usize {
    if info_table_size <= 255 {
        1
    } else {
        2
    }
}

/// Reduce a stored frame index to a keyed frame.
///
/// *Reference* (`teAnimation.cs`): `frame = abs(index) % InfoTableSize`.
/// Returns `None` for a zero modulus rather than dividing by zero.
pub fn normalize_frame_index(raw: i32, info_table_size: usize) -> Option<u16> {
    if info_table_size == 0 {
        return None;
    }
    Some((raw.unsigned_abs() as usize % info_table_size) as u16)
}

/// Read `count` stored frame indices at `off`, returning the keys and the
/// offset just past them.
///
/// Ports `teAnimation.ReadIndices` (`E:/CASCReader/OWLib/TankLib/teAnimation.cs`,
/// behaviour only): the width comes from [`frame_index_width`], each index is
/// reduced by [`normalize_frame_index`]. Returns `None` on a short buffer
/// rather than panicking.
pub fn read_frame_indices(
    bytes: &[u8],
    off: usize,
    count: usize,
    info_table_size: usize,
) -> Option<(Vec<u16>, usize)> {
    if info_table_size == 0 {
        return None;
    }
    let width = frame_index_width(info_table_size);
    if off + width * count > bytes.len() {
        return None;
    }
    let mut keys = Vec::with_capacity(count);
    for i in 0..count {
        let raw: i32 = if width == 1 {
            bytes[off + i] as i32
        } else {
            i16::from_le_bytes([bytes[off + i * 2], bytes[off + i * 2 + 1]]) as i32
        };
        keys.push(normalize_frame_index(raw, info_table_size)?);
    }
    Some((keys, off + width * count))
}

/// Bytes per sample for a track under a flag word.
///
/// Ports the reference's format selection (`teAnimation.cs`, `ReadRotation` /
/// `ReadPosition` with `InfoTableFlags.LegacyPositionFormat` /
/// `NewRotationCompression`): old rotation and scale are 6 bytes, new rotation
/// is 8, legacy position is 12, new position is 10 on the wire.
pub fn track_sample_stride(track: TrackKind, flags: TrackFlags) -> usize {
    match track {
        TrackKind::Rotation => {
            if flags.new_rotation() {
                8
            } else {
                6
            }
        }
        TrackKind::Scale => 6,
        TrackKind::Position => {
            if flags.legacy_position() {
                12
            } else {
                10
            }
        }
    }
}

/// One decoded sample of a keyed track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackSample {
    Position([f32; 3]),
    Rotation([f32; 4]),
    Scale([f32; 3]),
}

/// Decode one sample of `track` at `off`, honouring `flags`.
///
/// Ports the reference's per-track read (`teAnimation.cs`, behaviour only):
/// old 6-byte or new 8-byte rotation, f16÷32 (new, 10-byte wire) or f32
/// (legacy, 12-byte) position, ÷1024 scale. Returns `None` on a short buffer
/// or a misaligned rotation read rather than guessing.
pub fn decode_track_sample(
    bytes: &[u8],
    off: usize,
    track: TrackKind,
    flags: TrackFlags,
) -> Option<TrackSample> {
    match track {
        TrackKind::Rotation => {
            if flags.new_rotation() {
                if off + 8 > bytes.len() {
                    return None;
                }
                decode_packed_rotation_new(bytes, off).map(TrackSample::Rotation)
            } else {
                decode_packed_rotation(bytes, off).map(TrackSample::Rotation)
            }
        }
        TrackKind::Position => {
            let wire = if flags.legacy_position() { 12 } else { 10 };
            if off + wire > bytes.len() {
                return None;
            }
            Some(TrackSample::Position(decode_packed_position(
                bytes,
                off,
                flags.legacy_position(),
            )))
        }
        TrackKind::Scale => {
            if off + 6 > bytes.len() {
                return None;
            }
            Some(TrackSample::Scale(decode_packed_scale(bytes, off)))
        }
    }
}

/// One keyed track decoded through the kind dispatch: the keys plus one sample
/// per key.
///
/// Ports the reference's per-bone loop (`teAnimation.cs`, behaviour only):
/// read `count` indices, reduce each to `abs(index) % InfoTableSize`, then read
/// one value per key at `data_at + i * stride`. The caller supplies the index
/// and data offsets — this decodes slices, it does not hunt for them — so every
/// path is testable with synthetic bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyedTrack {
    pub track: TrackKind,
    pub keys: Vec<u16>,
    pub samples: Vec<TrackSample>,
}

/// Decode a keyed track from an index array and a value array.
///
/// * `index_at` — offset of `count` stored frame indices (width from
///   [`frame_index_width`]).
/// * `data_at` — offset of `count` packed samples at
///   [`track_sample_stride`] bytes each.
/// * `info_table_size` — the frame modulus ([`frame_key_modulus`]).
///
/// Returns `None` when either array runs past the buffer or a sample misaligns.
pub fn decode_keyed_track(
    bytes: &[u8],
    index_at: usize,
    data_at: usize,
    count: usize,
    track: TrackKind,
    flags: TrackFlags,
    info_table_size: usize,
) -> Option<KeyedTrack> {
    let (keys, _) = read_frame_indices(bytes, index_at, count, info_table_size)?;
    let stride = track_sample_stride(track, flags);
    if data_at + stride * count > bytes.len() {
        return None;
    }
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        samples.push(decode_track_sample(
            bytes,
            data_at + i * stride,
            track,
            flags,
        )?);
    }
    Some(KeyedTrack {
        track,
        keys,
        samples,
    })
}

// ---- the reference-layout track tables ------------------------------------
//
// Family `0x00020400` — the installed **first-person** family (7 samples on
// this install; the measured idle `0x0a0000000000c15a`, its three same-shape
// siblings and three short clips) — is laid out exactly as the reference
// implementation's per-bone `InfoTable` says, at a place the reference's own
// header calls `SomeTableOffset`: the section table's largest named file
// offset. (*Reference* layout, verified *install* — probed sample by sample in
// `dev/clip_fp.py` / `dev/clip_fp_lanes.py`, outputs in `D:/ow-dumps/`.)
//
// Table, stride 32 bytes:
// `{u16 scale_count, u16 position_count, u16 rotation_count; u16 InfoTableFlags;
//   i32 scale_indices, position_indices, rotation_indices;
//   i32 scale_data, position_data, rotation_data}` — every offset scaled ×4
// from the table's own start. Index entries are u8 while
// `info_table_size = floor(fps·duration) + 1 ≤ 255` else i16 (`ReadIndices`),
// frame = `abs(index) % info_table_size`; data land at
// `table + offset·4` with the sample size from the flags
// (`0x200` legacy 12-B f32 position else 10-B f16÷32; `0x400` 8-B new rotation
// else 6-B old; scale 3×u16÷1024, `0x100` unimplemented → rejected).
//
// What is measured per table closes earlier stories along the way: the "bone
// table" of preceding passes (168 entries, gaps at 8 and 82) **is track 0's
// position-index list** (frames 0..170 minus two skipped keys, count 169 =
// its `position_count`), and the "12-byte cadence" the value-area probes saw
// (one 6-byte constant among varying u16 lanes) is the legacy f32 position
// samples themselves.
// How the tables pair onto rig bones is **not read yet** (the retarget) — the
// decoder reports tables in file order without naming bones.

/// The one family this layout is measured on so far.
pub const REFERENCE_TABLE_FAMILY: u32 = 0x0002_0400;
/// One track table: three u16 counts, one u16 flag word, six i32 offsets.
pub const REF_TRACK_TABLE: usize = 32;

/// Decode the reference-layout track tables of a clip, **given the whole
/// asset file** — not `Clip::body`, which starts after the section table this
/// reads the table base from (measured body start 0x90 on the FP family with
/// the tables in that same region).
///
/// Stops at the first table whose counts are all zero or exceed
/// `floor(fps·duration) + 1` (the measured terminator), or whose sample
/// landing would run past the file. A table with a count but no readable
/// track is skipped, not an error — the caller sees what decoded.
pub fn decode_reference_tables(bytes: &[u8]) -> Option<Vec<Vec<KeyedTrack>>> {
    let header = Header::read(bytes)?;
    let info_table_size = (header.fps as f32 * header.duration).floor() as usize + 1;
    // the largest file offset named in the section table is the track-table
    // region on this family (measured [8] on c15a, [8] on its siblings)
    let mut base = 0usize;
    for i in 0..10 {
        let o = SECTION_TABLE + i * 8;
        if o + 8 > bytes.len() {
            break;
        }
        let v = u64::from_le_bytes(bytes[o..o + 8].try_into().ok()?);
        if v != 0 && v != u64::MAX && (v as usize) < bytes.len() && v as usize > base {
            base = v as usize;
        }
    }
    if base == 0 {
        return None;
    }
    let u16_at = |o: usize| -> Option<u16> {
        bytes
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let i32_at = |o: usize| -> Option<i32> {
        bytes
            .get(o..o + 4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let land = |table_at: usize, off: i32| -> Option<usize> {
        if off < 0 {
            return None;
        }
        let p = table_at + (off as usize).checked_mul(4)?;
        if p < bytes.len() {
            Some(p)
        } else {
            None
        }
    };
    let mut tables = Vec::new();
    let mut at = base;
    while at + REF_TRACK_TABLE <= bytes.len() {
        let sc = u16_at(at)?;
        let pc = u16_at(at + 2)?;
        let rc = u16_at(at + 4)?;
        if (sc, pc, rc) == (0, 0, 0) || usize::from(sc.max(pc).max(rc)) > info_table_size {
            break;
        }
        let flags = TrackFlags(u16_at(at + 6)?);
        if flags.0 & 0x100 != 0 {
            // NewScaleCompression: the reference marks it unimplemented and
            // did not measure it reading either; reject rather than guess.
            break;
        }
        let mut tracks = Vec::new();
        if sc > 0 {
            if let (Some(ia), Some(da)) = (land(at, i32_at(at + 8)?), land(at, i32_at(at + 20)?)) {
                if let Some(t) = decode_keyed_track(
                    bytes,
                    ia,
                    da,
                    sc as usize,
                    TrackKind::Scale,
                    flags,
                    info_table_size,
                ) {
                    tracks.push(t);
                }
            }
        }
        if pc > 0 {
            if let (Some(ia), Some(da)) = (land(at, i32_at(at + 12)?), land(at, i32_at(at + 24)?)) {
                if let Some(t) = decode_keyed_track(
                    bytes,
                    ia,
                    da,
                    pc as usize,
                    TrackKind::Position,
                    flags,
                    info_table_size,
                ) {
                    tracks.push(t);
                }
            }
        }
        if rc > 0 {
            if let (Some(ia), Some(da)) = (land(at, i32_at(at + 16)?), land(at, i32_at(at + 28)?)) {
                if let Some(t) = decode_keyed_track(
                    bytes,
                    ia,
                    da,
                    rc as usize,
                    TrackKind::Rotation,
                    flags,
                    info_table_size,
                ) {
                    tracks.push(t);
                }
            }
        }
        tables.push(tracks);
        at += REF_TRACK_TABLE;
    }
    if tables.is_empty() {
        None
    } else {
        Some(tables)
    }
}

// ---- the blend-family card streams ----------------------------------------
//
// Family `0x0053XXXX` — the family the FP blend-tree set's clips carry
// (measured members `0x00533c00` and `0x00532400`; the low half is a per-clip
// flag word, so the gate keys on the **upper half** and records the members
// seen) — is a *record + card* layout, decompiled on the runtime side and
// wired on the file side this round (2026-10-18, *install*, c50 = `0x0a00000000000050`,
// c71 = `0x0a00000000000071`):
//
// * **83 records of 32 bytes** from the section table's largest named offset:
//   `{u16 S_count, P_count, R_count; u16 flags; u32 s_off, p_off, r_off}` —
//   one record per bone-track block (83 = the FP rig's track count).
// * Each record's three offsets are **pool-zone cursors in count order**, at
//   `record_at + off·4` — the *same record-relative ×4 rule* as the
//   reference-layout family's data offsets. Cursor order matches count order
//   and never decreases (`s ≤ p ≤ r` on every measured record).
// * After the records comes one **sequential card stream**; record k's card
//   starts at its `s_off` landing and ends at the next record's. Card size is
//   *driven by the record's own counts* (c50: 40 B/card at `S1 P1 R1`; c71
//   record 1: 248 B at `S1 P17 R1`).
// * Inside a card, in this exact order:
//   ```text
//   S keys (u8/i16 × width(info) × S_count) @ s_land
//   P keys                                  @ p_land   (stops 4-aligned gap)
//   R keys                                  @ r_land   (4-aligned gap after)
//   S samples  6 B  (3×u16÷1024) × S_count  @ 4-align after R keys
//   P samples 12 B (0x200 legacy f32) or 10 B (f16÷32) × P_count
//                                            @ 4-align after S samples
//   R samples  6 B (0x400 absent) or 8 B (0x400 new) × R_count
//                                            @ exactly after P samples
//   [pad to the next card]
//   ```
//   Verified footprint: c50 card = 3(1) keys + 3(4-pad) + 6 + pad + 12 +
//   6+2pad = **40 B**; c71's P17 card = 1 + 3 + 17 + 4(align-8 gap) + 1 +
//   3 + 6 + (2) + **17·12 = 204** + 6+2 = **248 B** — and the card-pair diff
//   proves exactly **204 B = the 17 legacy f32 triples differ** while
//   everything structural is equal ("equal is structure, differing is data").
// * The **rest pose decodes sample-exact** on c50 (pos `(0,0,0)`, scale
//   `(1,1,1)` = `0x0400×3 ÷1024`, rotation `{0xc000,0xc000,0x8000}` = identity),
//   and c71's 17-key track is a real sway (y ≈ 1.49–1.52 m constant, x/z
//   jitter). Sample count == key count, keys `0..=frames` mod `info`.
// * Flag word mapping shares `TrackFlags` with the reference layout
//   (`0x200` legacy position, `0x400` new rotation; `0x100` unimplemented →
//   rejected — no measured sample carries it).

/// The measured members of the blend-card family so far (upper half shared).
pub const BLEND_CARD_FAMILY: u32 = 0x0053_0000;
/// One blend record: three u16 counts, one u16 flag word, three u32 cursors.
pub const BLEND_RECORD: usize = 32;

pub fn is_blend_card_family(family: u32) -> bool {
    (family & 0xFFFF_0000) == BLEND_CARD_FAMILY
}

/// Decode the blend-family record/card stream of a clip, **given the whole
/// asset file**, mirroring [`decode_reference_tables`]' contract. The result
/// is one entry per record (per track block / per rig bone, positionally),
/// each holding up to three `KeyedTrack`s that decoded.
///
/// Returns `None` when the header is unreadable, the section table names no
/// in-file base, or no record decodes — never a partial guess.
/// One blend record's decoded envelope: the record row's own numbers plus
/// the tracks its card produced. The shipped pose reader consumes `tracks`;
/// the rest is diagnostic surface that would otherwise need the walk redone.
///
/// `lands` = the row's **six** u32 cursor words (`+0x08..+0x20`): the first
/// three (the keys) are what the decoder walks by, and the last three are
/// the *sample* lands the client reads — the shipped assumption of
/// fixed-order sample positions is the suspected bug, so the envelope keeps
/// all six for the readers that want to diff them.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendRecordLane {
    /// The record's row offset in the file.
    pub byte_at: usize,
    /// The record's `(S, P, R)` key counts.
    pub counts: (u16, u16, u16),
    /// The record's card flags word.
    pub flags: TrackFlags,
    /// All six of the row's u32 cursor words, in file order.
    pub lands: [u32; 6],
    /// The card stream's end: the next record's s-land (EOF for the last).
    pub card_end: usize,
    /// The tracks the card decoded (scale/position/rotation, up to three).
    pub tracks: Vec<KeyedTrack>,
}

pub fn decode_blend_records(bytes: &[u8]) -> Option<Vec<BlendRecordLane>> {
    let header = Header::read(bytes)?;
    if !is_blend_card_family(header.family) {
        return None;
    }
    let info_table_size = (header.fps as f32 * header.duration).floor() as usize + 1;
    // the records live at the section table's largest named offset (the [8]
    // cell on both measured clips)
    let mut base = 0usize;
    for i in 0..10 {
        let o = SECTION_TABLE + i * 8;
        if o + 8 > bytes.len() {
            break;
        }
        let v = u64::from_le_bytes(bytes[o..o + 8].try_into().ok()?);
        if v != 0 && v != u64::MAX && (v as usize) < bytes.len() && v as usize > base {
            base = v as usize;
        }
    }
    if base == 0 {
        return None;
    }
    let u16_at = |o: usize| -> Option<u16> {
        bytes
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |o: usize| -> Option<u32> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let align4 = |p: usize| p.div_ceil(4) * 4;
    // first pass: envelope the records and their cards
    let mut records: Vec<(usize, (u16, u16, u16), TrackFlags, [u32; 6])> = Vec::new();
    let mut at = base;
    while at + BLEND_RECORD <= bytes.len() {
        let sc = u16_at(at)?;
        let pc = u16_at(at + 2)?;
        let rc = u16_at(at + 4)?;
        if (sc, pc, rc) == (0, 0, 0) || usize::from(sc.max(pc).max(rc)) > info_table_size {
            break;
        }
        let flags = TrackFlags(u16_at(at + 6)?);
        if flags.0 & 0x100 != 0 {
            break;
        }
        let s = u32_at(at + 8)?;
        let p = u32_at(at + 12)?;
        let r = u32_at(at + 16)?;
        if !(s <= p && p <= r) {
            // the measured cursor invariant; a violated record is not this layout
            break;
        }
        let s_samp = u32_at(at + 20)?;
        let p_samp = u32_at(at + 24)?;
        let r_samp = u32_at(at + 28)?;
        records.push((at, (sc, pc, rc), flags, [s, p, r, s_samp, p_samp, r_samp]));
        at += BLEND_RECORD;
    }
    if records.is_empty() {
        return None;
    }
    // second pass: decode each record's card
    let key_width = frame_index_width(info_table_size);
    let mut out: Vec<BlendRecordLane> = Vec::with_capacity(records.len());
    for (i, (rec_at, (_sc, _pc, _rc), _flags, _lands)) in records.iter().enumerate() {
        let (sc, pc, rc) = records[i].1;
        let flags = records[i].2;
        let lands = records[i].3;
        // the lands are record-relative: rec_at + v*4 into the card stream
        let rec_land = |v: u32| -> Option<usize> {
            rec_at
                .checked_add(v as usize * 4)
                .filter(|p| *p < bytes.len())
        };
        let s_keys = match rec_land(lands[0]) {
            Some(p) => p,
            None => continue,
        };
        let card_end = records
            .get(i + 1)
            .and_then(|(next_at, _, _, next_lands)| next_at.checked_add(next_lands[0] as usize * 4))
            .filter(|p| *p <= bytes.len())
            .unwrap_or(bytes.len());
        let mut lane = BlendRecordLane {
            byte_at: *rec_at,
            counts: (sc, pc, rc),
            flags,
            lands,
            card_end,
            tracks: Vec::new(),
        };
        let Some(p_keys) = rec_land(lands[1]) else {
            out.push(lane);
            continue;
        };
        let Some(r_keys) = rec_land(lands[2]) else {
            out.push(lane);
            continue;
        };
        let mut tracks = Vec::new();
        let s_keys_end = s_keys.saturating_add(key_width * sc as usize);
        let r_keys_end = r_keys.saturating_add(key_width * rc as usize);
        if s_keys_end > card_end
            || r_keys_end > card_end
            || p_keys + key_width * pc as usize > card_end
        {
            lane.tracks = tracks;
            out.push(lane);
            continue;
        }
        // samples: S right after the R keys' 4-align, then P 4-aligned, then R unaligned
        let mut cursor = align4(r_keys_end);
        if sc > 0 && cursor + 6 * sc as usize <= card_end {
            let data_at = cursor;
            if let Some(t) = decode_keyed_track(
                bytes,
                s_keys,
                data_at,
                sc as usize,
                TrackKind::Scale,
                flags,
                info_table_size,
            ) {
                cursor = data_at + 6 * sc as usize;
                tracks.push(t);
            }
        }
        if pc > 0 {
            let data_at = align4(cursor);
            let stride = track_sample_stride(TrackKind::Position, flags);
            if data_at + stride * pc as usize <= card_end {
                if let Some(t) = decode_keyed_track(
                    bytes,
                    p_keys,
                    data_at,
                    pc as usize,
                    TrackKind::Position,
                    flags,
                    info_table_size,
                ) {
                    cursor = data_at + stride * pc as usize;
                    tracks.push(t);
                } else {
                    cursor = data_at;
                }
            }
        }
        if rc > 0 {
            let data_at = cursor;
            let stride = track_sample_stride(TrackKind::Rotation, flags);
            if data_at + stride * rc as usize <= card_end {
                if let Some(t) = decode_keyed_track(
                    bytes,
                    r_keys,
                    data_at,
                    rc as usize,
                    TrackKind::Rotation,
                    flags,
                    info_table_size,
                ) {
                    tracks.push(t);
                }
            }
        }
        lane.tracks = tracks;
        out.push(lane);
    }
    if out.iter().any(|t| !t.tracks.is_empty()) {
        Some(out)
    } else {
        None
    }
}

pub fn decode_blend_registers(bytes: &[u8]) -> Option<Vec<Vec<KeyedTrack>>> {
    let lanes = decode_blend_records(bytes)?;
    Some(lanes.into_iter().map(|l| l.tracks).collect())
}

/// The general row reader for the non-card kind-006 families.
///
/// The published reference (`teAnimation.cs` *published-check*) gives this as
/// family-agnostic: `BoneCount` contiguous 32-byte rows at the rows section,
/// and **all six lands resolved as `(long)offset * 4 + the row's own address`**
/// — the row-relative basis. The samples sit at the row's *own* data lands
/// (`lands[3..5]`), never at a computed cursor: that reading was tried by the
/// cursor route only for the card family, where both agree (measured
/// `landversus_probe`); the general probe (`rowswalk`, out3) measured on the
/// installed hero-3 blend-set corpus that
///
/// * the envelope invariants hold on every family tried (cards 6, walk
///   `0x00130404`×4, `0x00110404`, trio `0x000a0404`×2, zone `0x001f0404`,
///   `0x00140404`×3, item5 four families, slots 1/5 `0x004c2400/0x004c2c00`),
/// * rotations decode at **100 % unit quaternions row-relative** on all of
///   them (140–324 per clip) while the rows-base-relative basis decodes a
///   strict subset, so the row-relative basis is the measured one, and
/// * positions decode finite and plausible row-relative with the flag-gated
///   12-byte legacy / 10-byte new stride (`0x200`/`0x400` semantics).
///
/// The first-section words are the records' bone ids as on the cards (the
/// walk clips name the FP arm/mirror pairs 13/14, 27/28, 48/49/50, 79/80,
/// 126 directly — one more word than records, the same off-by-one the cards
/// carry), so [`BlendPose::decode`]'s pairing needs no family-specific rule.
pub fn decode_general_records(bytes: &[u8]) -> Option<Vec<BlendRecordLane>> {
    let header = Header::read(bytes)?;
    let info_table_size = (header.fps as f32 * header.duration).floor() as usize + 1;
    // The row table is the section table's largest in-file named offset — the
    // same rule the card decoder uses, measured to name the row base on every
    // family probed (the row table is always the LAST in the table since the
    // first section is the ids).
    let mut base = 0usize;
    for i in 0..10 {
        let o = SECTION_TABLE + i * 8;
        if o + 8 > bytes.len() {
            break;
        }
        let v = u64::from_le_bytes(bytes[o..o + 8].try_into().ok()?);
        if v != 0 && v != u64::MAX && (v as usize) < bytes.len() && v as usize > base {
            base = v as usize;
        }
    }
    if base < BODY {
        return None;
    }
    let u16_at = |o: usize| -> Option<u16> {
        bytes
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |o: usize| -> Option<u32> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // envelope: same content invariants as the card walk, no family gate
    let mut records: Vec<(usize, (u16, u16, u16), TrackFlags, [u32; 6])> = Vec::new();
    let mut at = base;
    while at + BLEND_RECORD <= bytes.len() {
        let sc = u16_at(at)?;
        let pc = u16_at(at + 2)?;
        let rc = u16_at(at + 4)?;
        if (sc, pc, rc) == (0, 0, 0) || usize::from(sc.max(pc).max(rc)) > info_table_size {
            break;
        }
        let flags = TrackFlags(u16_at(at + 6)?);
        if flags.0 & 0x100 != 0 {
            break;
        }
        let s = u32_at(at + 8)?;
        let p = u32_at(at + 12)?;
        let r = u32_at(at + 16)?;
        if !(s <= p && p <= r) {
            break;
        }
        let s_samp = u32_at(at + 20)?;
        let p_samp = u32_at(at + 24)?;
        let r_samp = u32_at(at + 28)?;
        // every land must point inside the file at the row-relative basis
        let land_in = |v: u32| {
            at.checked_add(v as usize * 4)
                .is_some_and(|p| p < bytes.len())
        };
        if !(land_in(s)
            && land_in(p)
            && land_in(r)
            && land_in(s_samp)
            && land_in(p_samp)
            && land_in(r_samp))
        {
            break;
        }
        records.push((at, (sc, pc, rc), flags, [s, p, r, s_samp, p_samp, r_samp]));
        at += BLEND_RECORD;
    }
    if records.is_empty() {
        return None;
    }
    let key_width = frame_index_width(info_table_size);
    let mut out = Vec::with_capacity(records.len());
    let mut any_track = false;
    for (i, &(rec_at, (sc, pc, rc), flags, lands)) in records.iter().enumerate() {
        // The reference's basis: every land adds to *this* row's address.
        let rec_land = |v: u32| -> Option<usize> {
            (v as usize)
                .checked_mul(4)
                .and_then(|o| rec_at.checked_add(o))
                .filter(|p| *p < bytes.len())
        };
        let card_end = records
            .get(i + 1)
            .and_then(|(next_at, _, _, next_lands)| {
                (next_lands[0] as usize)
                    .checked_mul(4)
                    .and_then(|o| next_at.checked_add(o))
            })
            .filter(|p| *p <= bytes.len())
            .unwrap_or(bytes.len());
        let mut lane = BlendRecordLane {
            byte_at: rec_at,
            counts: (sc, pc, rc),
            flags,
            lands,
            card_end,
            tracks: Vec::new(),
        };
        // Samples at the row's OWN data lands, each lane independently.
        for (key_land, data_land, count, kind) in [
            (lands[0], lands[3], sc as usize, TrackKind::Scale),
            (lands[1], lands[4], pc as usize, TrackKind::Position),
            (lands[2], lands[5], rc as usize, TrackKind::Rotation),
        ] {
            if count == 0 {
                continue;
            }
            let Some(keys_at) = rec_land(key_land) else {
                continue;
            };
            let Some(data_at) = rec_land(data_land) else {
                continue;
            };
            let stride = track_sample_stride(kind, flags);
            if data_at + stride * count > bytes.len() || keys_at + key_width * count > card_end {
                continue;
            }
            if let Some(t) =
                decode_keyed_track(bytes, keys_at, data_at, count, kind, flags, info_table_size)
            {
                lane.tracks.push(t);
            }
        }
        any_track |= !lane.tracks.is_empty();
        out.push(lane);
    }
    if any_track {
        Some(out)
    } else {
        None
    }
}

/// A blend-family clip decoded into a pose source the renderer queries.
///
/// **The pairing rule measured:** the clip's own **first-section word i**
/// names record i's bone id (`Skeleton::ids` matches), so a record → skin
/// bone join looks the id up in the model skeleton. Some records name ids
/// the model's skeleton doesn't carry (the clip-side extras, e.g. the cast
/// `82, 81, 48, 27 ...` set in c50 with the last section word doubling as a
/// size annotation `567`) — those map to no skin bone and are the 11
/// surplus records the bind gate measures (71 of 83 resolve, the rest =
/// skeleton-side or metadata).
#[derive(Debug, Clone)]
pub struct BlendPose {
    records: Vec<Vec<KeyedTrack>>,
    ids: Vec<u32>,
    frames: u16,
    fps: f32,
}

impl BlendPose {
    /// Decode a blend-family clip **from the whole asset file**.
    pub fn decode(file: &[u8]) -> Option<BlendPose> {
        let header = Header::read(file)?;
        if !is_blend_card_family(header.family) {
            // Non-card families ride the general row reader (published
            // reference: row-relative lands, samples at the row's own data
            // lands). The card path stays the one verified sample-exact.
            let lanes = decode_general_records(file)?;
            let regs: Vec<Vec<KeyedTrack>> = lanes.into_iter().map(|l| l.tracks).collect();
            let ids: Vec<u32> = (0..regs.len())
                .map(|i| u32::from_le_bytes(file[0x80 + i * 4..0x84 + i * 4].try_into().unwrap()))
                .collect();
            return Some(BlendPose {
                records: regs,
                ids,
                frames: header.frames,
                fps: header.fps as f32,
            });
        }
        let regs = decode_blend_registers(file)?;
        // The first-section words, one per record: section [1]'s offset times
        // the words list, indexed by the record's slot.
        let ids: Vec<u32> = (0..regs.len())
            .map(|i| u32::from_le_bytes(file[0x80 + i * 4..0x84 + i * 4].try_into().unwrap()))
            .collect();
        Some(BlendPose {
            records: regs,
            ids,
            frames: header.frames,
            fps: header.fps as f32,
        })
    }

    /// The clip's own bone id for record `record`.
    pub fn bone_id(&self, record: usize) -> Option<u32> {
        self.ids.get(record).copied()
    }

    pub fn records(&self) -> usize {
        self.records.len()
    }

    pub fn frames(&self) -> u16 {
        self.frames
    }

    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// The frame a time lands on (the same wrap rule `PoseClip::frame_at`
    /// uses: the client's `AnimTimeWrap` over `[0, frames]` with the flag
    /// clear).
    pub fn frame_at(&self, seconds: f32) -> f32 {
        let n = self.frames.max(1) as f32;
        anim_time_wrap([0.0, n, 0.0, 0.0], seconds * self.fps)
    }

    /// One record's `{rotation, translation}` at `seconds`, interpolated
    /// between the keyed samples (linear for positions/scales, slerp for
    /// rotations). `None` per component = leave that channel at bind.
    pub fn sample_record(
        &self,
        record: usize,
        seconds: f32,
    ) -> Option<(Option<Quat>, Option<Vec3>)> {
        let tracks = self.records.get(record)?;
        let frame = self.frame_at(seconds);
        let mut rotation = None;
        let mut translation = None;
        for t in tracks {
            let Some(k) = key_pair(&t.keys, frame) else {
                continue;
            };
            match (t.track, k) {
                (TrackKind::Position, KeyPair::Single(i)) => {
                    if let TrackSample::Position(p) = t.samples[i] {
                        translation = Some(Vec3::new(p[0], p[1], p[2]));
                    }
                }
                (TrackKind::Position, KeyPair::Between(a, b, f)) => {
                    if let (TrackSample::Position(p), TrackSample::Position(q)) =
                        (&t.samples[a], &t.samples[b])
                    {
                        // the client keeps the sample drives linear between keys; the
                        // position track's own time spacing is its frame list.
                        let v = [
                            p[0] + (q[0] - p[0]) * f,
                            p[1] + (q[1] - p[1]) * f,
                            p[2] + (q[2] - p[2]) * f,
                        ];
                        translation = Some(Vec3::new(v[0], v[1], v[2]));
                    }
                }
                (TrackKind::Rotation, KeyPair::Single(i)) => {
                    if let TrackSample::Rotation(q) = t.samples[i] {
                        rotation = Some(Quat::from_xyzw(q[0], q[1], q[2], q[3]));
                    }
                }
                (TrackKind::Rotation, KeyPair::Between(a, b, f)) => {
                    if let (TrackSample::Rotation(p), TrackSample::Rotation(q)) =
                        (&t.samples[a], &t.samples[b])
                    {
                        let px = slerp_quat(*p, *q, f);
                        rotation = Some(Quat::from_xyzw(px[0], px[1], px[2], px[3]));
                    }
                }
                _ => {}
            }
        }
        if rotation.is_none() && translation.is_none() {
            return None;
        }
        Some((rotation, translation))
    }

    /// The clipped *skin-bone* pose (record i) at `seconds`, where `Skin`
    /// hands back the matching `Skeleton::ids` index — the caller's own
    /// pairing: `Some((bone_index_in_skin, pose))` for every resolvable
    /// record.
    pub fn skin_pose(
        &self,
        record: usize,
        seconds: f32,
        skin_ids: &[u32],
    ) -> Option<(usize, (Option<Quat>, Option<Vec3>))> {
        let id = self.bone_id(record)?;
        let bone = skin_ids.iter().position(|x| *x == id)?;
        let pose = self.sample_record(record, seconds)?;
        Some((bone, pose))
    }
}

/// A keyed frame's neighbourhood: one key, or the pair around `frame`.
#[derive(Debug, Clone, Copy)]
enum KeyPair {
    Single(usize),
    Between(usize, usize, f32),
}

fn key_pair(keys: &[u16], frame: f32) -> Option<KeyPair> {
    if keys.is_empty() {
        return None;
    }
    if keys.len() == 1 {
        return Some(KeyPair::Single(0));
    }
    if frame <= keys[0] as f32 {
        return Some(KeyPair::Single(0));
    }
    let last = keys.len() - 1;
    if frame >= keys[last] as f32 {
        return Some(KeyPair::Single(last));
    }
    for k in 0..last {
        let a = keys[k] as f32;
        let b = keys[k + 1] as f32;
        if a <= frame && frame <= b {
            // never divide by zero; frame strictly between here
            let f = (frame - a) / (b - a).max(1.0);
            return Some(KeyPair::Between(k, k + 1, f.clamp(0.0, 1.0)));
        }
    }
    Some(KeyPair::Single(last))
}

#[cfg(test)]
mod reference_layout_tests {
    use super::*;

    /// A synthetic family-0x00020400 clip with the same regions the installed
    /// samples show, pinned to the measured constants: tables at the section
    /// table's largest offset, index bytes while `info_table_size ≤ 255`, a
    /// legacy-position sample of 12 bytes, a 6-byte rotation, and a scale of
    /// 922/1024 — the exact value read from the real idle `0x0a0000000000c15a`.
    fn synthetic_clip() -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        let put_u16 =
            |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let put_u32 =
            |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let put_i32 =
            |b: &mut [u8], o: usize, v: i32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let put_f32 =
            |b: &mut [u8], o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put_u32(&mut b, 0x00, 0x0002_0400);
        put_u16(&mut b, 0x0C, 170);
        put_u16(&mut b, 0x0E, 30);
        put_f32(&mut b, 0x14, 170.0 / 30.0);
        // section table: [1] = 0x80 (a metadata block), [8] = 0x90 (the tables)
        let put_q = |b: &mut [u8], o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
        put_q(&mut b, SECTION_TABLE + 8, 0x80);
        put_q(&mut b, SECTION_TABLE + 8 * 8, 0x90);
        // table 0 @0x90: S1 P3 R2, legacy positions + old rotations (0x0217)
        put_u16(&mut b, 0x90, 1);
        put_u16(&mut b, 0x92, 3);
        put_u16(&mut b, 0x94, 2);
        put_u16(&mut b, 0x96, 0x0217);
        put_i32(&mut b, 0x98, 0x10); // scale idx  -> 0x90+0x40 = 0xd0
        put_i32(&mut b, 0x9c, 0x11); // pos idx    -> 0xd4
        put_i32(&mut b, 0xa0, 0x14); // rot idx    -> 0xe0
        put_i32(&mut b, 0xa4, 0x2e); // scale data -> 0x90+0xb8 = 0x148
        put_i32(&mut b, 0xa8, 0x31); // pos data   -> 0x154
        put_i32(&mut b, 0xac, 0x34); // rot data   -> 0x160
                                     // scale index (1 byte = key 5), position indices (keys 0, 2, 4),
                                     // rotation indices (keys 3, 7)
        b[0xd0] = 5;
        b[0xd4] = 0;
        b[0xd5] = 2;
        b[0xd6] = 4;
        b[0xe0] = 3;
        b[0xe1] = 7;
        // scale data: 922 == 0x039a -> 0.9003906
        for k in 0..3 {
            b[0x148 + k * 2] = 0x9a;
            b[0x149 + k * 2] = 0x03;
        }
        // position data: three legacy 12-byte f32 samples (y bob, z constant —
        // the shape the real idle shows)
        put_f32(&mut b, 0x154, 0.0);
        put_f32(&mut b, 0x158, 0.01);
        put_f32(&mut b, 0x15c, 5e-5);
        put_f32(&mut b, 0x160, -0.005);
        put_f32(&mut b, 0x164, 0.02);
        put_f32(&mut b, 0x168, 5e-5);
        put_f32(&mut b, 0x16c, 0.005);
        put_f32(&mut b, 0x170, -0.018);
        put_f32(&mut b, 0x174, 5e-5);
        // rotation data: the two real quats the format test pins
        // (`tests/rotation_format.rs`: 0x9f00 0x3367 0xe73c, and a unit quat
        // with axis 0: 0x8000, 0x8000, 0x8000)
        b[0x178..0x17e].copy_from_slice(&[0x00, 0x9f, 0x67, 0x33, 0x3c, 0xe7]);
        b[0x17e..0x184].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
        // table 1 @0xb0: one key each, same flags
        put_u16(&mut b, 0xb0, 1);
        put_u16(&mut b, 0xb2, 1);
        put_u16(&mut b, 0xb4, 1);
        put_u16(&mut b, 0xb6, 0x0277);
        put_i32(&mut b, 0xb8, 0x20); // scale idx -> 0xb0+0x80 = 0x130
        put_i32(&mut b, 0xbc, 0x21); // pos idx  -> 0x134
        put_i32(&mut b, 0xc0, 0x22); // rot idx  -> 0x138
        put_i32(&mut b, 0xc4, 0x23); // data     -> 0x13c
        put_i32(&mut b, 0xc8, 0x23);
        put_i32(&mut b, 0xcc, 0x23); // (single-sample table: index once, data reused)
        b[0x130] = 9;
        b[0x134] = 11;
        b[0x138] = 13;
        b
    }

    #[test]
    fn the_tables_decode_and_stop_at_the_terminator() {
        let b = synthetic_clip();
        let tables = decode_reference_tables(&b).expect("tables decode");
        assert_eq!(tables.len(), 2);
    }

    #[test]
    fn positions_remain_legacy_and_match_the_written_samples() {
        let b = synthetic_clip();
        let tables = decode_reference_tables(&b).unwrap();
        let pos = tables[0]
            .iter()
            .find(|t| t.track == TrackKind::Position)
            .expect("position track");
        assert_eq!(pos.keys, [0, 2, 4]);
        for (i, s) in pos.samples.iter().enumerate() {
            let y = match s {
                TrackSample::Position(p) => p[1],
                _ => panic!("wrong kind"),
            };
            let written = [0.01f32, 0.02, -0.018][i];
            assert!((y - written).abs() < 1e-9, "y {i}: {y}");
        }
    }

    #[test]
    fn rotations_and_the_measured_scale_decode() {
        let b = synthetic_clip();
        let tables = decode_reference_tables(&b).unwrap();
        let rot = tables[0]
            .iter()
            .find(|t| t.track == TrackKind::Rotation)
            .expect("rotation track");
        assert_eq!(rot.keys, [3, 7]);
        for q in &rot.samples {
            let quat = match q {
                TrackSample::Rotation(r) => r,
                _ => panic!("wrong kind"),
            };
            let m = (quat.iter().map(|x| x * x).sum::<f32>()).sqrt();
            assert!((m - 1.0).abs() < 1e-2, "|q| = {m}");
        }
        let scl = tables[0]
            .iter()
            .find(|t| t.track == TrackKind::Scale)
            .expect("scale track");
        assert_eq!(scl.keys, [5]);
        for s in &scl.samples {
            if let TrackSample::Scale(v) = s {
                assert!(
                    (v[0] - 922.0 / 1024.0).abs() < 1e-6
                        && (v[1] - 922.0 / 1024.0).abs() < 1e-6
                        && (v[2] - 922.0 / 1024.0).abs() < 1e-6,
                    "scale {v:?}"
                );
            }
        }
    }

    #[test]
    fn the_unimplemented_scale_compression_rejects_the_table() {
        let mut b = synthetic_clip();
        // flags bit 0x100 = NewScaleCompression, unimplemented in the reference
        // and unmeasured in any install sample: the decoder stops rather than
        // guess.
        b[0x96] = 0x17;
        b[0x97] = 0x03;
        assert!(decode_reference_tables(&b).is_none());
    }

    #[test]
    fn an_out_of_range_table_count_stops_the_scan() {
        // the position-count field over the info-table bound cannot be a table
        let mut b = synthetic_clip();
        b[0x92] = 0xff;
        b[0x93] = 0xff;
        assert!(decode_reference_tables(&b).is_none());
    }
}

#[cfg(test)]
mod blend_card_tests {
    use super::*;

    /// A synthetic family-0x00533c00 clip in the measured card layout. The
    /// records sit at the section table's largest offset and the card stream
    /// follows them; a card starts at its record's S-land and ends at the next
    /// record's. The record walk terminates inside the card stream because
    /// every card's S-land opens on a `0x00` key (frame 0) inside the u16-count
    /// window the walk reads.
    ///
    /// Two records: rec 0 `S1 P1 R1` flag 0x0277 (legacy position, old
    /// rotation — the rest pose's shape); rec 1 `S1 P2 R1` flag 0x0677 (new
    /// rotation).
    fn synthetic_blend_clip() -> Vec<u8> {
        let mut b = vec![0u8; 0x300];
        let put_u16 =
            |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let put_u32 =
            |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let put_f32 =
            |b: &mut [u8], o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let put_q = |b: &mut [u8], o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
        put_u32(&mut b, 0x00, 0x0053_3c00);
        put_u16(&mut b, 0x0C, 5);
        put_u16(&mut b, 0x0E, 30);
        put_f32(&mut b, 0x14, 5.0 / 30.0);
        put_q(&mut b, SECTION_TABLE + 8, 0x80);
        put_q(&mut b, SECTION_TABLE + 8 * 8, 0x90);
        // records at 0x90 (2 x 32 B; the region ends at 0xd0)
        put_u16(&mut b, 0x90, 1);
        put_u16(&mut b, 0x92, 1);
        put_u16(&mut b, 0x94, 1);
        put_u16(&mut b, 0x96, 0x0277);
        put_u32(&mut b, 0x98, 0x10); // s = 16 u32 slots -> land 0xd0
        put_u32(&mut b, 0x9c, 0x11); // p -> 0xd4
        put_u32(&mut b, 0xa0, 0x12); // r -> 0xd8
        put_u16(&mut b, 0xb0, 1);
        put_u16(&mut b, 0xb2, 2);
        put_u16(&mut b, 0xb4, 1);
        put_u16(&mut b, 0xb6, 0x0677);
        put_u32(&mut b, 0xb8, 0x12); // s = 18 -> land 0xb0+0x48 = 0xf8
        put_u32(&mut b, 0xbc, 0x13); // p -> 0xfc
        put_u32(&mut b, 0xc0, 0x15); // r -> 0x104
                                     // card 0 = 0xd0..0xf8
        b[0xd0] = 0x00; // S key (frame 0)
        b[0xd4] = 0x00; // P key
        b[0xd8] = 0x00; // R key
        put_u16(&mut b, 0xdc, 0x0400);
        put_u16(&mut b, 0xde, 0x0400);
        put_u16(&mut b, 0xe0, 0x0400);
        put_f32(&mut b, 0xe4, 0.0);
        put_f32(&mut b, 0xe8, 1.4986);
        put_f32(&mut b, 0xec, 0.0);
        b[0xf0..0xf6].copy_from_slice(&[0x00, 0xc0, 0x00, 0xc0, 0x00, 0x80]);
        // card 1 = 0xf8..EOF
        b[0xf8] = 0x00;
        b[0xfc] = 0x00;
        b[0xfd] = 0x02;
        b[0x104] = 0x05;
        put_u16(&mut b, 0x108, 0x0400);
        put_u16(&mut b, 0x10a, 0x0400);
        put_u16(&mut b, 0x10c, 0x0400);
        put_f32(&mut b, 0x110, 0.001);
        put_f32(&mut b, 0x114, 1.5006);
        put_f32(&mut b, 0x118, 0.004);
        put_f32(&mut b, 0x11c, 0.002);
        put_f32(&mut b, 0x120, 1.5120);
        put_f32(&mut b, 0x124, 0.009);
        put_u16(&mut b, 0x128, 0x0000);
        put_u16(&mut b, 0x12a, 0x8000);
        put_u16(&mut b, 0x12c, 0x0000);
        put_u16(&mut b, 0x12e, 0x8000);
        b
    }

    #[test]
    fn blend_cards_decode_and_rest_pose_identifies() {
        let b = synthetic_blend_clip();
        let regs = decode_blend_registers(&b).expect("blend decode");
        assert_eq!(regs.len(), 2);
        let scale0 = regs[0]
            .iter()
            .find(|t| t.track == TrackKind::Scale)
            .expect("scale track 0");
        assert_eq!(scale0.keys, [0]);
        for s in &scale0.samples {
            match s {
                TrackSample::Scale(v) => assert!(
                    (v[0] - 1.0).abs() < 1e-6
                        && (v[1] - 1.0).abs() < 1e-6
                        && (v[2] - 1.0).abs() < 1e-6,
                    "scale {v:?}"
                ),
                _ => panic!("wrong kind"),
            }
        }
        let pos0 = regs[0]
            .iter()
            .find(|t| t.track == TrackKind::Position)
            .expect("pos track 0");
        assert_eq!(pos0.keys, [0]);
        match pos0.samples[0] {
            TrackSample::Position(p) => assert_eq!(p[1], 1.4986),
            _ => panic!("wrong kind"),
        }
        let rot0 = regs[0]
            .iter()
            .find(|t| t.track == TrackKind::Rotation)
            .expect("rot track 0");
        assert_eq!(rot0.keys, [0]);
        match rot0.samples[0] {
            TrackSample::Rotation(q) => assert_eq!(q, [0.0, 0.0, 0.0, 1.0]),
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn the_two_key_card_carries_two_samples() {
        let b = synthetic_blend_clip();
        let regs = decode_blend_registers(&b).unwrap();
        let pos1 = regs[1]
            .iter()
            .find(|t| t.track == TrackKind::Position)
            .expect("pos track of rec 1");
        assert_eq!(pos1.keys, [0, 2]);
        assert_eq!(pos1.samples.len(), 2);
        match pos1.samples[0] {
            TrackSample::Position(p) => assert_eq!(p[1], 1.5006),
            _ => panic!("wrong kind"),
        }
        match pos1.samples[1] {
            TrackSample::Position(p) => assert_eq!(p[1], 1.5120),
            _ => panic!("wrong kind"),
        }
        let rot1 = regs[1]
            .iter()
            .find(|t| t.track == TrackKind::Rotation)
            .expect("rot track of rec 1");
        assert_eq!(rot1.keys, [5]);
        let m = match rot1.samples[0] {
            TrackSample::Rotation(q) => (q.iter().map(|x| x * x).sum::<f32>()).sqrt(),
            _ => panic!("wrong kind"),
        };
        assert!((m - 1.0).abs() < 0.02, "new-rot |q| = {m}");
    }

    #[test]
    fn the_family_gate_rejects_other_families() {
        let mut b = synthetic_blend_clip();
        // 0x00020400-le (the reference family's word): the first u32's upper
        // half is what the gate keys on, so overwrite bytes 2..4.
        b[2] = 0x02;
        b[3] = 0x00;
        assert!(decode_blend_registers(&b).is_none());
    }
}
#[cfg(test)]
mod general_rows_tests {
    use super::*;

    /// A synthetic walk-family clip (`0x00130404`, 3 frames, 2 rows) laid out
    /// the way the installed walk clips read: the ids at 0x80, the row table
    /// at the section table's largest named offset, and all samples addressed
    /// by their row's own tail lands. Positions in the 12-byte legacy form
    /// (flag 0x200), rotations in the 6-byte old form.
    fn synthetic_general_walk_clip() -> Vec<u8> {
        let mut b = vec![0u8; 0x140];
        let put_u16 =
            |b: &mut Vec<u8>, o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let put_u32 =
            |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let put_f32 =
            |b: &mut Vec<u8>, o: usize, v: f32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let put_q =
            |b: &mut Vec<u8>, o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
        put_u32(&mut b, 0x00, 0x0013_0404);
        put_u16(&mut b, 0x0c, 3);
        put_u16(&mut b, 0x0e, 30);
        put_f32(&mut b, 0x14, 3.0 / 30.0);
        put_q(&mut b, SECTION_TABLE + 8, 0x80); // section [0]: first-section ids
        put_q(&mut b, SECTION_TABLE + 8 * 8, 0x90); // largest named: the rows table
                                                    // first-section ids: two bone ids (the walk clips' arm pair shape)
        put_u32(&mut b, 0x80, 13);
        put_u32(&mut b, 0x84, 27);
        // row A at 0x90: pc=1, rc=2, legacy position, old rotation
        put_u16(&mut b, 0x90, 0);
        put_u16(&mut b, 0x92, 1);
        put_u16(&mut b, 0x94, 2);
        put_u16(&mut b, 0x96, 0x0234);
        put_u32(&mut b, 0x98, 0x00); // s-key (unused, sc=0)
        put_u32(&mut b, 0x9c, 0x10); // p-key -> 0x90+0x40 = 0xd0
        put_u32(&mut b, 0xa0, 0x14); // r-keys -> 0xe0
        put_u32(&mut b, 0xa4, 0x00); // s-data (unused)
        put_u32(&mut b, 0xa8, 0x11); // p-data -> 0xd4
        put_u32(&mut b, 0xac, 0x13); // r-data -> 0xe4
                                     // row B at 0xb0: sc=1, pc=0, rc=1. Its lands are relative to its OWN
                                     // address (0xb0), inside the 0x100..0x140 data region.
        put_u16(&mut b, 0xb0, 1);
        put_u16(&mut b, 0xb2, 0);
        put_u16(&mut b, 0xb4, 1);
        put_u16(&mut b, 0xb6, 0x0216);
        put_u32(&mut b, 0xb8, 0x14); // s-key -> 0xb0+80 = 0x100
        put_u32(&mut b, 0xbc, 0x14); // p-key (unused)
        put_u32(&mut b, 0xc0, 0x19); // r-key -> 0x114
        put_u32(&mut b, 0xc4, 0x15); // s-data -> 0x104
        put_u32(&mut b, 0xc8, 0x00); // p-data (unused)
        put_u32(&mut b, 0xcc, 0x1a); // r-data -> 0x118
                                     // row A's card
        b[0xd0] = 0; // p-key: frame 0
        put_f32(&mut b, 0xd4, 0.1);
        put_f32(&mut b, 0xd8, 0.0);
        put_f32(&mut b, 0xdc, -0.05);
        b[0xe0] = 0; // r-key frame 0
        b[0xe1] = 3; // r-key frame 3
        b[0xe4..0xea].copy_from_slice(&[0x00, 0xc0, 0x00, 0xc0, 0x00, 0x80]);
        b[0xea..0xf0].copy_from_slice(&[0x00, 0xc0, 0x00, 0xc0, 0x00, 0x80]);
        // row B's card
        b[0x100] = 0; // s-key frame 0
        let scale = [0x0400u16, 0x0400, 0x0400];
        for (i, v) in scale.iter().enumerate() {
            put_u16(&mut b, 0x104 + i * 2, *v);
        }
        b[0x114] = 3; // r-key frame 3
        put_u16(&mut b, 0x118, 0x0000);
        put_u16(&mut b, 0x11a, 0x0000);
        put_u16(&mut b, 0x11c, 0x8000);
        b
    }

    #[test]
    fn the_card_gate_missing_means_the_card_decoder_skips() {
        assert!(decode_blend_registers(&synthetic_general_walk_clip()).is_none());
    }

    #[test]
    fn the_general_reader_decodes_two_rows_id_pairs_and_tracks() {
        let b = synthetic_general_walk_clip();
        let regs = decode_general_records(&b).expect("general decode");
        assert_eq!(regs.len(), 2);
        assert_eq!(regs[0].byte_at, 0x90);
        assert_eq!(regs[0].counts, (0, 1, 2));
        assert_eq!(regs[1].counts, (1, 0, 1));
        let pos = regs[0]
            .tracks
            .iter()
            .find(|t| t.track == TrackKind::Position)
            .expect("position track");
        assert_eq!(pos.keys, [0]);
        match pos.samples[0] {
            TrackSample::Position(p) => {
                assert!((p[0] - 0.1).abs() < 1e-4, "pos x = {}", p[0]);
                assert!((p[2] + 0.05).abs() < 1e-4, "pos z = {}", p[2]);
            }
            _ => panic!("wrong kind"),
        }
        let rot = regs[0]
            .tracks
            .iter()
            .find(|t| t.track == TrackKind::Rotation)
            .expect("rotation track");
        assert_eq!(rot.keys, [0, 3]);
        assert_eq!(rot.samples.len(), 2);
        let scale = regs[1]
            .tracks
            .iter()
            .find(|t| t.track == TrackKind::Scale)
            .expect("scale track");
        match scale.samples[0] {
            TrackSample::Scale(v) => assert_eq!(v, [1.0, 1.0, 1.0]),
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn blend_pose_consumes_the_general_family_and_pairs_ids() {
        let b = synthetic_general_walk_clip();
        let pose = BlendPose::decode(&b).expect("blend pose over a non-card family");
        assert_eq!(pose.records(), 2);
        assert_eq!(pose.frames(), 3);
        assert_eq!(pose.bone_id(0), Some(13));
        assert_eq!(pose.bone_id(1), Some(27));
        // at t=0 the first record's translation is its frame-0 sample
        let (_, t) = pose.sample_record(0, 0.0).expect("rec0 sample");
        let tr = t.expect("translation");
        assert!((tr[0] - 0.1).abs() < 1e-4, "tr x = {}", tr[0]);
        // mid-clip the two identity rotations interpolate to identity
        let (r, _) = pose.sample_record(0, 0.05).expect("rec0 mid");
        let _ = r;
        // row B carries a scale + rotation track and no position: at mid-clip
        // its pose pairs an identity rotation with the absent translation
        let (r, t) = pose.sample_record(1, 0.05).expect("rec1 mid");
        assert!(t.is_none(), "row B has no position track");
        let _rot = r.expect("row B rotation");
    }
}
#[cfg(test)]
mod kind_dispatch_tests {
    use super::*;

    #[test]
    fn the_table_names_all_31_kinds() {
        assert_eq!(KIND_TABLE.len(), 31);
        for (i, info) in KIND_TABLE.iter().enumerate() {
            assert_eq!(info.kind as usize, i + 1);
        }
    }

    #[test]
    fn resident_and_inline_handlers_are_where_the_capture_says() {
        // v2b addresses; live = v2b − 0x1B1A10000 in the practice-range session.
        assert_eq!(kind_info(2).unwrap().handler_v2b, Some(0x7ff7_c58e_a110));
        assert_eq!(
            kind_info(2).unwrap().residency,
            KindResidency::ResidentHandler
        );
        assert_eq!(kind_info(10).unwrap().handler_v2b, Some(0x7ff7_c58e_bbf0));
        assert_eq!(
            kind_info(10).unwrap().residency,
            KindResidency::ResidentHandler
        );
        assert_eq!(kind_info(3).unwrap().handler_v2b, Some(0x7ff7_c58d_f6cd));
        assert_eq!(
            kind_info(3).unwrap().residency,
            KindResidency::InlineHandler
        );
        assert_eq!(kind_info(18).unwrap().handler_v2b, Some(0x7ff7_c58d_f854));
        assert_eq!(
            kind_info(18).unwrap().residency,
            KindResidency::InlineHandler
        );
    }

    #[test]
    fn stub_and_cold_kinds_decode_as_expected() {
        for k in [6u8, 9, 11, 12, 13, 14, 16, 17, 20, 21, 30] {
            let info = kind_info(k).unwrap();
            assert_eq!(info.residency, KindResidency::DefaultStub, "kind {k}");
            assert_eq!(info.handler_v2b, Some(KIND_DEFAULT_STUB_V2B), "kind {k}");
        }
        for k in [1u8, 4, 5, 7, 8, 15, 19, 31] {
            assert_eq!(
                kind_info(k).unwrap().residency,
                KindResidency::ColdUnverified,
                "kind {k}"
            );
        }
        // Kind 0 underflows the dispatcher's `kind − 1` index; 32 is past the
        // table. Both land on the default stub in the client, which this port
        // reports as "no entry".
        assert!(kind_info(0).is_none());
        assert!(kind_info(32).is_none());
        assert!(kind_info(255).is_none());
    }

    #[test]
    fn the_kind_byte_is_read_at_record_plus_0x0e() {
        let mut record = vec![0u8; RECORD_STRIDE_20];
        record[RECORD_KIND_OFFSET] = 10;
        assert_eq!(read_record_kind(&record, 0), Some(10));
        assert_eq!(
            kind_info(read_record_kind(&record, 0).unwrap())
                .unwrap()
                .kind,
            10
        );
        // a short buffer is an error, not a panic
        assert!(read_record_kind(&record[..RECORD_KIND_OFFSET], 0).is_none());
    }

    #[test]
    fn the_stride_is_gated_per_family() {
        assert_eq!(record_stride_for_family(0x006d_2000), Some(20));
        assert_eq!(record_stride_for_family(0x0053_3c00), Some(20));
        // The 0x002a0604 singleton, the first-person family, and garbage:
        // unmeasured, so no walk.
        assert_eq!(record_stride_for_family(0x002a_0604), None);
        assert_eq!(record_stride_for_family(0x0002_0400), None);
        assert_eq!(record_stride_for_family(0), None);
    }

    #[test]
    fn the_old_rotation_sample_is_pinned_to_the_install() {
        // The exact six bytes of clip 0x0a0000000000005e at file 0x119c —
        // the same vector `rotation_format.rs` pins.
        let sample = [0x00u8, 0x9f, 0x67, 0x33, 0x3c, 0xe7];
        let flags = TrackFlags(0);
        let decoded = decode_track_sample(&sample, 0, TrackKind::Rotation, flags);
        match decoded {
            Some(TrackSample::Rotation(q)) => {
                let m = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                assert!((m - 1.0).abs() < 1e-3, "not unit: {q:?}");
            }
            other => panic!("expected a rotation, got {other:?}"),
        }
        assert_eq!(track_sample_stride(TrackKind::Rotation, flags), 6);
    }

    #[test]
    fn the_new_rotation_identity_decodes() {
        // a = b = c = 0x8000, d = 0: every mantissa centred, axis 0,
        // so (x, y, z) = (0, 0, 0) and the derived w = 1.
        let mut sample = vec![0u8; 8];
        for o in [0, 2, 4] {
            sample[o..o + 2].copy_from_slice(&0x8000u16.to_le_bytes());
        }
        let flags = TrackFlags(TrackFlags::NEW_ROTATION_COMPRESSION);
        assert_eq!(track_sample_stride(TrackKind::Rotation, flags), 8);
        match decode_track_sample(&sample, 0, TrackKind::Rotation, flags) {
            Some(TrackSample::Rotation(q)) => {
                assert!((q[0] - 1.0).abs() < 1e-6, "w: {q:?}");
                assert!(q[1].abs() < 1e-6 && q[2].abs() < 1e-6 && q[3].abs() < 1e-6);
            }
            other => panic!("expected a rotation, got {other:?}"),
        }
        // and a short buffer is an error, not a panic
        assert!(decode_track_sample(&sample[..7], 0, TrackKind::Rotation, flags).is_none());
    }

    #[test]
    fn position_and_scale_codecs_match_the_reference_arithmetic() {
        let plain = TrackFlags(0);
        let legacy = TrackFlags(TrackFlags::LEGACY_POSITION_FORMAT);
        assert_eq!(track_sample_stride(TrackKind::Position, legacy), 12);
        assert_eq!(track_sample_stride(TrackKind::Position, plain), 10);
        assert_eq!(track_sample_stride(TrackKind::Scale, plain), 6);

        // legacy position: three f32 verbatim
        let mut raw = vec![0u8; 12];
        for (o, v) in [0usize, 4, 8].iter().zip([1.5f32, -2.0, 0.25]) {
            raw[*o..*o + 4].copy_from_slice(&v.to_le_bytes());
        }
        match decode_track_sample(&raw, 0, TrackKind::Position, legacy) {
            Some(TrackSample::Position(p)) => {
                assert!((p[0] - 1.5).abs() < 1e-6);
                assert!((p[1] + 2.0).abs() < 1e-6);
                assert!((p[2] - 0.25).abs() < 1e-6);
            }
            other => panic!("expected a position, got {other:?}"),
        }

        // new position: f16 1.0 is 0x3C00, decoded ÷ 32
        let mut packed = vec![0u8; 10];
        packed[0..2].copy_from_slice(&0x3C00u16.to_le_bytes());
        packed[2..4].copy_from_slice(&0x4000u16.to_le_bytes());
        packed[4..6].copy_from_slice(&0xBC00u16.to_le_bytes());
        match decode_track_sample(&packed, 0, TrackKind::Position, plain) {
            Some(TrackSample::Position(p)) => {
                assert!((p[0] - 1.0 / 32.0).abs() < 1e-7, "x: {}", p[0]);
                assert!((p[1] - 2.0 / 32.0).abs() < 1e-7, "y: {}", p[1]);
                assert!((p[2] + 1.0 / 32.0).abs() < 1e-7, "z: {}", p[2]);
            }
            other => panic!("expected a position, got {other:?}"),
        }

        // scale: three u16 ÷ 1024
        let mut sc = vec![0u8; 6];
        for (o, v) in [0usize, 2, 4].iter().zip([1024u16, 2048, 512]) {
            sc[*o..*o + 2].copy_from_slice(&v.to_le_bytes());
        }
        match decode_track_sample(&sc, 0, TrackKind::Scale, plain) {
            Some(TrackSample::Scale(s)) => {
                assert!((s[0] - 1.0).abs() < 1e-6);
                assert!((s[1] - 2.0).abs() < 1e-6);
                assert!((s[2] - 0.5).abs() < 1e-6);
            }
            other => panic!("expected a scale, got {other:?}"),
        }
    }

    #[test]
    fn frame_indices_use_u8_below_256_and_i16_above() {
        assert_eq!(frame_index_width(21), 1);
        assert_eq!(frame_index_width(255), 1);
        assert_eq!(frame_index_width(256), 2);
        // frame = abs(index) % size
        assert_eq!(normalize_frame_index(7, 21), Some(7));
        assert_eq!(normalize_frame_index(-7, 21), Some(7));
        assert_eq!(normalize_frame_index(22, 21), Some(1));
        assert!(normalize_frame_index(1, 0).is_none());

        // u8 path: keys 1, 3, 5 in a 21-frame clip
        let (keys, next) = read_frame_indices(&[1u8, 3, 5, 0], 0, 3, 21).unwrap();
        assert_eq!(keys, vec![1, 3, 5]);
        assert_eq!(next, 3);
        // i16 path: a 300-frame clip stores 0x012C little-endian, and -1
        // reduces to abs(-1) % 301 = 1
        let wide = [0x2Cu8, 0x01, 0xFF, 0xFF];
        let (keys, next) = read_frame_indices(&wide, 0, 2, 301).unwrap();
        assert_eq!(keys, vec![300, 1]);
        assert_eq!(next, 4);
        // short buffers fail rather than panic
        assert!(read_frame_indices(&[1u8, 2], 0, 3, 21).is_none());
        assert!(read_frame_indices(&wide[..3], 0, 2, 301).is_none());
    }

    #[test]
    fn a_keyed_rotation_track_decodes_end_to_end() {
        // Three u8 indices then three old-rotation samples: the reference's
        // per-bone loop (indices → abs % size → one value per key) on
        // synthetic bytes.
        let mut bytes = vec![1u8, 3, 5];
        let zeros = [0x00u8, 0x80, 0x00, 0x80, 0x00, 0x80]; // centred → identity-ish
        for _ in 0..3 {
            bytes.extend_from_slice(&zeros);
        }
        let flags = TrackFlags(0);
        let track = decode_keyed_track(&bytes, 0, 3, 3, TrackKind::Rotation, flags, 21).unwrap();
        assert_eq!(track.track, TrackKind::Rotation);
        assert_eq!(track.keys, vec![1, 3, 5]);
        assert_eq!(track.samples.len(), 3);
        for s in &track.samples {
            match *s {
                TrackSample::Rotation(q) => {
                    let m = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                    assert!((m - 1.0).abs() < 1e-3);
                }
                other => panic!("expected rotations, got {other:?}"),
            }
        }
        // a truncated value array fails rather than guessing
        assert!(decode_keyed_track(&bytes[..8], 0, 3, 3, TrackKind::Rotation, flags, 21).is_none());
    }

    #[test]
    fn the_value_search_is_bounded_by_the_window() {
        // One record whose value array sits 300 bytes past its key list.
        // Three bones: the finder needs a run of at least 3. The body is
        // 0xFF outside the record (NaN, never a sample): with a zero fill,
        // phase-shifted windows over the real samples also decode as unit
        // quaternions and the hunt legitimately stops early.
        let mut body = vec![0xFFu8; 1024];
        body[0] = 1;
        body[1] = 2;
        body[2] = 3;
        body[3] = 0; // bone-list terminator
        body[4] = 1;
        body[5] = 3;
        body[6] = 0; // key-list terminator
        let at = 7 + 300;
        let quats: [(f32, f32, f32); 2] =
            [(0.0, 0.2, 0.0), (0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0)];
        for (i, (x, y, z)) in quats.iter().copied().enumerate() {
            let o = at + i * 16;
            body[o..o + 4].copy_from_slice(&x.to_le_bytes());
            body[o + 4..o + 8].copy_from_slice(&y.to_le_bytes());
            body[o + 8..o + 12].copy_from_slice(&z.to_le_bytes());
        }
        // a 64-byte window cannot reach it; the default 8 KB window can
        assert!(decode_channels_with_window(&body, 21, 64).is_empty());
        let found = decode_channels_with_window(&body, 21, CHANNEL_VALUE_WINDOW);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].values_offset, at);
    }

    #[test]
    fn the_prebuilt_index_agrees_with_the_scan() {
        // The index holds exactly the unit-sample offsets, sorted. The body
        // is 0xFF (NaN, never a sample) with one isolated triple of 0.5s —
        // zeros would read as valid samples beside real data, so they are
        // not the fill here.
        let mut body = vec![0xFFu8; 64];
        body[4..8].copy_from_slice(&0.5f32.to_le_bytes());
        body[8..12].copy_from_slice(&0.5f32.to_le_bytes());
        body[12..16].copy_from_slice(&0.5f32.to_le_bytes());
        let index = build_unit_sample_index(&body);
        assert_eq!(index, vec![4]);
        assert!(index.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn slerp_holds_endpoints_and_sweeps_uniformly() {
        // 90° about Y: identity to (0, sin45°, 0, cos45°); the midpoint is
        // exactly 45° about Y, i.e. (0, sin22.5°, 0, cos22.5°).
        let a = [0.0f32, 0.0, 0.0, 1.0];
        let b = [
            0.0f32,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
        ];
        let ends0 = slerp_quat(a, b, 0.0);
        let ends1 = slerp_quat(a, b, 1.0);
        for (got, want) in ends0.iter().zip(a) {
            assert!((got - want).abs() < 1e-6);
        }
        for (got, want) in ends1.iter().zip(b) {
            assert!((got - want).abs() < 1e-5);
        }
        let mid = slerp_quat(a, b, 0.5);
        assert!((mid[1] - 0.3826834).abs() < 1e-4, "y: {}", mid[1]);
        assert!((mid[3] - 0.9238795).abs() < 1e-4, "w: {}", mid[3]);
        let m = (mid[0] * mid[0] + mid[1] * mid[1] + mid[2] * mid[2] + mid[3] * mid[3]).sqrt();
        assert!((m - 1.0).abs() < 1e-6);
        // antipodal input takes the short arc, not the long way round
        let neg = [-b[0], -b[1], -b[2], -b[3]];
        let mid_neg = slerp_quat(a, neg, 0.5);
        for (got, want) in mid_neg.iter().zip(mid) {
            assert!((got - want).abs() < 1e-5);
        }
    }
}

#[cfg(test)]
mod time_wrap_tests {
    use super::*;

    /// The node's `float[4]` range: `[min, max/period, slot2, flag-as-bits]`.
    fn range(min: f32, max: f32, flag: u32) -> [f32; 4] {
        [min, max, 0.0, f32::from_bits(flag)]
    }

    /// The harness unit's own discriminating cases, flag bit 0 clear: the
    /// read's clamp branch, `maxss(minss(t, max), min)`.
    #[test]
    fn the_flag_clear_branch_is_the_clients_clamp() {
        let cases: [(f32, f32, u32, f32); 12] = [
            (0.0, 2.0, 0b00, 5.5),           // -> 2.0
            (0.0, 2.0, 0b10, 5.5),           // bit 1 only: still clamp -> 2.0
            (0.0, 2.0, 0b10, -1.5),          // negative -> 0.0
            (0.0, 2.0, 0b10, 2.0),           // exactly max -> 2.0
            (1.0, 4.0, 0b10, 7.25),          // nonzero min -> 4.0
            (0.0, 2.0, 0b100, 5.5),          // bit 2 only: clamp -> 2.0
            (0.0, 2.0, 0x8000_0000, 5.5),    // sign bit only: clamp -> 2.0
            (-2.0, 2.0, 0b10, 3.5),          // negative min -> 2.0
            (0.0, 1.0, 0b10, 0.4),           // inside -> 0.4
            (0.0, 2.0, 0b10, 0.0),           // zero -> 0.0
            (0.0, 0.0, 0b10, 5.5),           // degenerate min == max -> 0.0
            (0.0, 2.0, 0b10, f32::INFINITY), // +inf -> 2.0
        ];
        let want = [
            2.0f32, 2.0, 0.0, 2.0, 4.0, 2.0, 2.0, 2.0, 0.4, 0.0, 0.0, 2.0,
        ];
        for ((min, max, flag, t), want) in cases.into_iter().zip(want) {
            assert_eq!(
                anim_time_wrap(range(min, max, flag), t),
                want,
                "clamp range=({min}, {max}) flag={flag:#b} t={t}"
            );
        }
        // NaN falls through to the second source of minss, so the clamp lands
        // on the max rather than propagating NaN.
        assert_eq!(anim_time_wrap(range(0.0, 2.0, 0b10), f32::NAN), 2.0);
    }

    /// Flag bit 0 set: the read's truncating modular wrap,
    /// `t - (float)(int)(t / max) * max`.
    #[test]
    fn the_flag_set_branch_is_the_clients_truncating_wrap() {
        let cases: [(f32, f32, u32, f32); 8] = [
            (0.0, 2.0, 0b01, 5.5),           // 5.5 - 2*2 = 1.5
            (0.0, 2.0, 0b11, 5.5),           // both bits: wrap -> 1.5
            (0.0, 2.0, 0xff_ff_ff_ff, 5.5),  // all bits: wrap -> 1.5
            (0.0, 2.0, 0b01, -1.5),          // -1.5 - 0*2 = -1.5 (truncating!)
            (0.0, 2.0, 0b01, 7.25),          // 7.25 - 3*2 = 1.25
            (0.0, 2.0, 0b01, 2.0),           // exactly one period -> 0.0
            (0.0, 2.0, 0b01, 0.0),           // 0.0
            (0.0, 2.0, 0b01, f32::INFINITY), // cvttss2si(inf) = INT_MIN -> inf
        ];
        let want = [1.5f32, 1.5, 1.5, -1.5, 1.25, 0.0, 0.0, f32::INFINITY];
        for ((min, max, flag, t), want) in cases.into_iter().zip(want) {
            assert_eq!(
                anim_time_wrap(range(min, max, flag), t),
                want,
                "wrap range=({min}, {max}) flag={flag:#b} t={t}"
            );
        }
        // A negative t stays negative: `rem_euclid` and `fract` would both give
        // +0.5 here, which is the false start the audit records.
        assert!((anim_time_wrap(range(0.0, 2.0, 0b01), -1.5) - -1.5).abs() < 1e-6);
        assert!(anim_time_wrap(range(0.0, 2.0, 0b01), f32::NAN).is_nan());
    }

    /// The wrap test is `& 1`, not `& 3`: with bit 1 set and bit 0 clear the
    /// client clamps. This is the mask-widen mutant the harness fixture exists
    /// to kill.
    #[test]
    fn only_flag_bit_zero_selects_the_wrap() {
        let t = 5.5;
        assert_eq!(anim_time_wrap(range(0.0, 2.0, 0b00), t), 2.0);
        assert_eq!(anim_time_wrap(range(0.0, 2.0, 0b10), t), 2.0);
        assert_eq!(anim_time_wrap(range(0.0, 2.0, 0b01), t), 1.5);
    }

    /// `frame_at` routes through the read wrap with the clip's own range and
    /// the flag clear, and carries no `+1`: the stored keys are 0-based.
    #[test]
    fn frame_at_clamps_to_the_clip_range() {
        let clip = PoseClip {
            tracks: vec![],
            frames: 21,
            fps: 30.0,
        };
        assert_eq!(clip.frame_at(0.0), 0.0);
        assert_eq!(clip.frame_at(1.0 / 30.0), 1.0);
        assert_eq!(clip.frame_at(0.5), 15.0);
        // overshoot clamps to the last keyed frame; negative clamps to the first
        assert_eq!(clip.frame_at(1.0), 21.0);
        assert_eq!(clip.frame_at(-1.0), 0.0);

        let blend = BlendPose {
            records: vec![],
            ids: vec![],
            frames: 21,
            fps: 30.0,
        };
        assert_eq!(blend.frame_at(0.5), 15.0);
        assert_eq!(blend.frame_at(1.0), 21.0);
        assert_eq!(blend.frame_at(-1.0), 0.0);
    }
}

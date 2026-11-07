//! Effect assets: the `EFCT` root and its effect-component chunks.
//!
//! An effect (asset types `0x00D`, `0x04A`, `0x08E`, `0x08F`, `0x0D4`, `0x12B`)
//! is an ordinary chunked asset whose root chunk is `EFCT`. Its children are
//! "components": a model, an entity, a sound, a nested effect, and so on, each
//! one a chunk tag of its own. This module reproduces the client's effect-chunk
//! layer, operation for operation:
//!
//! * the **kind table** at `0x143610490` — 0x35 `{u32 tag, u32 slot}` records,
//!   the first empty — which turns a chunk tag into the small slot number the
//!   runtime record carries ([`kind_of`]);
//! * the **component builder** `FUN_142e4a780`, whose per-tag branch copies a
//!   fixed set of payload fields into a runtime record. The field widths and
//!   payload offsets here are those copies (`payload = record + 0x10`), so a
//!   component struct is only built when the payload holds every byte the
//!   client reads ([`Chunk::model`] and friends);
//! * the **fixup** `FUN_142e4e8e0`, which gates some tags on their chunk
//!   version before converting stored offsets to pointers. Which tag a version
//!   compare belongs to was not recovered reliably, so the module pins the
//!   install's version per tag ([`OBSERVED_VERSIONS`]) instead of claiming a
//!   gate.
//!
//! `EFCT`'s own version is 5 and its payload is at least 0x80 bytes on the
//! install; the builder's prologue reads it up to `+0x66` ([`Header`]).
//!
//! # What the client does not read
//!
//! Five tags the reference reader (OWLib's `TankLib/Chunks`) treats as effect
//! chunks have **no read site in this image**: `ECMP` (timing), `ECSS` (shader
//! setup), `ECAT`/`ESHT` (hardpoints) and `ECCV`. Their four-byte tag constant
//! occurs nowhere in the image in either byte order, and neither the kind table
//! nor the builder's tag chain contains them. Their payloads are therefore
//! editor-side data, not client behaviour, and this module keeps them as raw
//! chunks rather than claiming a layout ([`Chunk::payload`]).
//!
//! The reference's component structs are also **stale**: every one of them
//! describes a pre-2.x payload. `ECMD` is 32 bytes here, not 24; `ECSO` 32, not
//! 8; `ECPR` 112, not 40; `ECEN` 80, not 16; `ECEC` 64, not 20; `ECVS` 48, not
//! 8; `ECRR` 64, not 8; `ECEF` 32, not 8. The install agrees with the client on
//! every one of those sizes (`tests/effect_real.rs`).
//!
//! Evidence classes: *read* for the kind table, the builder's copies and the
//! fixup's version gates (addresses above); *install* for the payload sizes and
//! the GUID types each named field carries. See `docs/formats/effects.md`.

use std::fmt;

use crate::chunks::{self, Chunks};

/// The root chunk tag of an effect asset.
pub const ROOT: [u8; 4] = *b"EFCT";
/// The root chunk version every effect on the install carries.
pub const ROOT_VERSION: u16 = 5;
/// Smallest `EFCT` payload the builder's prologue can read: its last read is
/// the byte at `+0x66`. Every effect on the install is larger (the smallest is
/// 0x80), so this is a guard, not a measurement.
pub const ROOT_MIN: usize = 0x67;

/// The kind table at `0x143610490`: `{u32 tag, u32 slot}` per record, 0x35
/// records of which the first is empty (`{0, 0}`). The slot is the value the
/// runtime record's `+0xb8` carries; `0x35` means "no kind" (the builder's
/// search bound is `0x35`, the count plus one).
///
/// The slots are 1..=0x34 in table order, so this array is indexed by
/// `slot - 1`.
pub const KIND_TABLE: [[u8; 4]; 0x34] = [
    *b"EELB", *b"ECEE", *b"ECPR", *b"ECMD", *b"ECGP", *b"TCCT", *b"ECSO", *b"ECEF", *b"ECDL",
    *b"ECRR", *b"ECLT", *b"ECDF", *b"ECRE", *b"ECLE", *b"ECRM", *b"ECRP", *b"ECRB", *b"ECEA",
    *b"EcCS", *b"ECGS", *b"ECES", *b"ECEN", *b"TCCE", *b"ECME", *b"ECFS", *b"ECEC", *b"ECEP",
    *b"EcCB", *b"PRTR", *b"SHBR", *b"ECVS", *b"ECTL", *b"ECTS", *b"ECDJ", *b"ECCm", *b"ECSQ",
    *b"ECI_", *b"ECSC", *b"TCER", *b"TCGM", *b"TCCO", *b"ECSH", *b"TCMF", *b"TCDF", *b"TCMB",
    *b"TCSE", *b"BLND", *b"ECLF", *b"ECKB", *b"TCCC", *b"CLBN", *b"CLTN",
];

/// The value a chunk's kind takes when its tag is not in [`KIND_TABLE`].
pub const NO_KIND: u8 = 0x35;

/// The version every asset on the install carries for a tag, measured over
/// every effect asset of types `0x00D`, `0x04A`, `0x08E`, `0x08F`, `0x0D4` and
/// `0x12B` (`tests/effect_real.rs`).
///
/// These are **install** facts, not client constants: the fixup
/// `FUN_142e4e8e0` gates some tags with a version compare, but which tag a
/// compare belongs to was not recovered reliably enough to publish a per-tag
/// gate. The version is still worth pinning: it is constant for every tag on
/// this install, so a build that changes one is visible.
pub const OBSERVED_VERSIONS: [(&[u8; 4], u16); 20] = [
    (b"EFCT", 5),
    (b"ECMD", 1),
    (b"ECSO", 0),
    (b"ECEF", 0),
    (b"ECEN", 2),
    (b"ECEC", 0),
    (b"ECVS", 1),
    (b"ECRR", 0),
    (b"ECPR", 2),
    (b"ECME", 3),
    (b"ECSS", 2),
    (b"ECMP", 0),
    (b"ECAT", 0),
    (b"ESHT", 0),
    (b"ECLT", 2),
    (b"ECSH", 4),
    (b"ECRE", 2),
    (b"ECRM", 2),
    (b"ECTL", 0),
    (b"ECTS", 0),
];

#[derive(Debug)]
pub enum Error {
    /// The asset is not a chunked-data container.
    NotChunked(chunks::Error),
    /// The root chunk is not `EFCT`.
    NotAnEffect([u8; 4]),
    /// The root header is shorter than the fields the builder reads.
    ShortHeader { need: usize, got: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotChunked(e) => write!(f, "{e}"),
            Error::NotAnEffect(tag) => write!(
                f,
                "root chunk is {:?}, not an effect",
                String::from_utf8_lossy(tag)
            ),
            Error::ShortHeader { need, got } => {
                write!(f, "EFCT payload is {got} bytes; the client reads {need}")
            }
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// The slot [`KIND_TABLE`] gives a tag, or [`NO_KIND`].
pub fn kind_of(tag: &[u8; 4]) -> u8 {
    match KIND_TABLE.iter().position(|t| t == tag) {
        Some(i) => i as u8 + 1,
        None => NO_KIND,
    }
}

/// The tag a slot stands for, if the slot is in range.
pub fn tag_of(kind: u8) -> Option<[u8; 4]> {
    if kind == 0 || kind > 0x34 {
        return None;
    }
    Some(KIND_TABLE[kind as usize - 1])
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// The `EFCT` root's payload.
///
/// The builder's prologue copies fields out of it into a runtime header:
/// `u64`s at `+0x10`, `+0x18` and `+0x20` (the first two are also the sources
/// of two arrays it copies), `u32`s at `+0x18`..`+0x40`, `u16`s from `+0x28`
/// to `+0x58`, a `u32` at `+0x5c` and bytes at `+0x60`..`+0x66`.
///
/// No field's meaning was read, so this type exposes the raw payload and the
/// handful of widths the client uses. `+0x66` is the one named accessor: it is
/// the count the client multiplies by four to place the first array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header<'a> {
    pub payload: &'a [u8],
}

/// Offsets the builder's prologue reads out of the `EFCT` payload.
pub mod header {
    /// Two `u64` fields the prologue copies arrays from.
    pub const ARRAYS: [usize; 2] = [0x10, 0x18];
    /// The `u64` fields the prologue copies into the runtime header.
    pub const QWORDS: [usize; 3] = [0x10, 0x18, 0x20];
    /// The `u32` fields it copies (`+0x40` first, then `+0x18`..`+0x34`).
    pub const DWORDS: [usize; 5] = [0x40, 0x18, 0x1c, 0x20, 0x24];
    /// The byte the client multiplies by four: the first array's length.
    pub const COUNT: usize = 0x66;
}

impl<'a> Header<'a> {
    /// The payload byte at `+0x66`, the one count the client uses arithmetically.
    pub fn array_count(&self) -> u8 {
        self.payload.get(header::COUNT).copied().unwrap_or(0)
    }

    pub fn u16_at(&self, at: usize) -> Option<u16> {
        u16_at(self.payload, at)
    }

    pub fn u32_at(&self, at: usize) -> Option<u32> {
        u32_at(self.payload, at)
    }

    pub fn u64_at(&self, at: usize) -> Option<u64> {
        u64_at(self.payload, at)
    }
}

/// One chunk of an effect, with the slot the client's kind table gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub tag: [u8; 4],
    /// The tag's slot in [`KIND_TABLE`], or [`NO_KIND`].
    pub kind: u8,
    /// The chunk record's version (`record + 0xe`), which `FUN_142e4e8e0`
    /// checks for the tags it gates.
    pub version: u16,
    /// Nesting depth: 0 is a child of the `EFCT` root.
    pub depth: u8,
    /// The chunk's own bytes, without the record.
    pub payload: &'a [u8],
}

impl Chunk<'_> {
    /// The four-character tag as text.
    pub fn name(&self) -> String {
        self.tag.iter().map(|&b| b as char).collect()
    }

    /// A model component (`ECMD`, version 1).
    ///
    /// The client copies three `u64`s and two bytes. The GUID types on the
    /// install pin the meaning: `+0x00` is always type `0x00C` (model), `+0x08`
    /// type `0x01A` (look), `+0x10` type `0x006` (animation). The old reader
    /// called the field at `+0x08` "ModelLook" and stopped at 24 bytes; that was
    /// right about the name and wrong about the size (32 bytes here).
    pub fn model(&self) -> Option<Model> {
        if &self.tag != b"ECMD" || self.payload.len() < 0x1a {
            return None;
        }
        Some(Model {
            model: u64_at(self.payload, 0x00)?,
            look: u64_at(self.payload, 0x08)?,
            animation: u64_at(self.payload, 0x10)?,
            unknown_18: *self.payload.get(0x18)?,
            unknown_19: *self.payload.get(0x19)?,
        })
    }

    /// A nested effect (`ECEF`, version 0). `+0x00` is the effect GUID; the
    /// client also copies five bytes from `+0x08`.
    pub fn effect(&self) -> Option<NestedEffect> {
        if &self.tag != b"ECEF" || self.payload.len() < 0x0d {
            return None;
        }
        Some(NestedEffect {
            effect: u64_at(self.payload, 0x00)?,
            unknown_08: *self.payload.get(0x08)?,
            unknown_09: *self.payload.get(0x09)?,
            unknown_0a: *self.payload.get(0x0a)?,
            unknown_0b: *self.payload.get(0x0b)?,
            unknown_0c: *self.payload.get(0x0c)?,
        })
    }

    /// An entity component (`ECEN`, version 2).
    ///
    /// Five `u64`s, a `u32`, nine `u16`s (`+0x2c..+0x3e`) and seven bytes
    /// (`+0x3e..+0x45`). `+0x00` is always type `0x003` on the install, which is
    /// the reference's `Entity`; `+0x08` is type `0x01C` (`Identifier` in the
    /// reference, at `+0x08` there too).
    pub fn entity(&self) -> Option<Entity> {
        if &self.tag != b"ECEN" || self.payload.len() < 0x45 {
            return None;
        }
        let mut words = [0u16; 9];
        for (i, w) in words.iter_mut().enumerate() {
            *w = u16_at(self.payload, 0x2c + i * 2)?;
        }
        let mut bytes = [0u8; 7];
        bytes.copy_from_slice(self.payload.get(0x3e..0x45)?);
        Some(Entity {
            entity: u64_at(self.payload, 0x00)?,
            unknown_08: u64_at(self.payload, 0x08)?,
            unknown_10: u64_at(self.payload, 0x10)?,
            unknown_18: u64_at(self.payload, 0x18)?,
            unknown_20: u64_at(self.payload, 0x20)?,
            unknown_28: u32_at(self.payload, 0x28)?,
            words,
            bytes,
        })
    }

    /// An entity-control component (`ECEC`, version 0).
    ///
    /// `+0x00` is type `0x006` (animation) and `+0x10` type `0x01C`
    /// (`Identifier`). The reference put the identifier at `+0x08`; it moved.
    pub fn entity_control(&self) -> Option<EntityControl> {
        if &self.tag != b"ECEC" || self.payload.len() < 0x38 {
            return None;
        }
        let mut words = [0u16; 9];
        for (i, w) in words.iter_mut().enumerate() {
            *w = u16_at(self.payload, 0x24 + i * 2)?;
        }
        Some(EntityControl {
            animation: u64_at(self.payload, 0x00)?,
            unknown_08: u64_at(self.payload, 0x08)?,
            identifier: u64_at(self.payload, 0x10)?,
            unknown_18: u64_at(self.payload, 0x18)?,
            unknown_20: u32_at(self.payload, 0x20)?,
            words,
            unknown_36: u16_at(self.payload, 0x36)?,
        })
    }

    /// A sound component (`ECSO`, version 0).
    ///
    /// `+0x00` is type `0x02C` on every asset measured; the reference calls it
    /// `Sound`. The 8 bytes at `+0x08` and the `u16`/byte at `+0x10`/`+0x12`
    /// have no name. The two bytes at `+0x12` also take part of the chunk
    /// header's flag byte (`record + 0x18` bit 3), which is why the install
    /// value at `+0x12` alone is not the whole story.
    pub fn sound(&self) -> Option<Sound> {
        if &self.tag != b"ECSO" || self.payload.len() < 0x13 {
            return None;
        }
        Some(Sound {
            sound: u64_at(self.payload, 0x00)?,
            unknown_08: u64_at(self.payload, 0x08)?,
            unknown_10: u16_at(self.payload, 0x10)?,
            unknown_12: *self.payload.get(0x12)?,
        })
    }

    /// A voice-stimulus component (`ECVS`, version 1). `+0x00` is type `0x078`
    /// on every asset measured.
    pub fn voice_stimulus(&self) -> Option<VoiceStimulus> {
        if &self.tag != b"ECVS" || self.payload.len() < 0x28 {
            return None;
        }
        Some(VoiceStimulus {
            stimulus: u64_at(self.payload, 0x00)?,
            unknown_08: u32_at(self.payload, 0x08)?,
            unknown_0c: u32_at(self.payload, 0x0c)?,
            unknown_10: u32_at(self.payload, 0x10)?,
            unknown_14: *self.payload.get(0x14)?,
            unknown_18: u64_at(self.payload, 0x18)?,
            unknown_20: u64_at(self.payload, 0x20)?,
        })
    }

    /// A ribbon-renderer component (`ECRR`, version 0).
    ///
    /// `+0x00` is type `0x00C` (model) on every asset measured and `+0x08` is a
    /// `u32`. The rest is a run of `u16` reads with two gaps: `+0x0c..+0x1a`
    /// (seven), `+0x1c..+0x24` (four), `+0x26..+0x38` (nine) and one byte at
    /// `+0x38`. The payload is 64 bytes; nothing past `+0x38` is copied.
    pub fn ribbon(&self) -> Option<Ribbon> {
        if &self.tag != b"ECRR" || self.payload.len() < 0x39 {
            return None;
        }
        Some(Ribbon {
            model: u64_at(self.payload, 0x00)?,
            unknown_08: u32_at(self.payload, 0x08)?,
            head: {
                let mut w = [0u16; 7];
                for (i, x) in w.iter_mut().enumerate() {
                    *x = u16_at(self.payload, 0x0c + i * 2)?;
                }
                w
            },
            middle: {
                let mut w = [0u16; 4];
                for (i, x) in w.iter_mut().enumerate() {
                    *x = u16_at(self.payload, 0x1c + i * 2)?;
                }
                w
            },
            tail: {
                let mut w = [0u16; 9];
                for (i, x) in w.iter_mut().enumerate() {
                    *x = u16_at(self.payload, 0x26 + i * 2)?;
                }
                w
            },
            unknown_38: *self.payload.get(0x38)?,
        })
    }
}

/// `ECMD`: the model a component plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// `+0x00`, type `0x00C`.
    pub model: u64,
    /// `+0x08`, type `0x01A`.
    pub look: u64,
    /// `+0x10`, type `0x006`; zero on assets that play no animation.
    pub animation: u64,
    pub unknown_18: u8,
    pub unknown_19: u8,
}

/// `ECEF`: a nested effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestedEffect {
    /// `+0x00`, type `0x00D`.
    pub effect: u64,
    pub unknown_08: u8,
    pub unknown_09: u8,
    pub unknown_0a: u8,
    pub unknown_0b: u8,
    pub unknown_0c: u8,
}

/// `ECEN`: an entity component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entity {
    /// `+0x00`, type `0x003`.
    pub entity: u64,
    /// `+0x08`, type `0x01C`.
    pub unknown_08: u64,
    /// `+0x10`, type `0x0A6` when non-zero.
    pub unknown_10: u64,
    /// `+0x18`, type `0x0CE` when non-zero.
    pub unknown_18: u64,
    pub unknown_20: u64,
    pub unknown_28: u32,
    /// `+0x2c..+0x3e`, nine `u16`s.
    pub words: [u16; 9],
    /// `+0x3e..+0x45`.
    pub bytes: [u8; 7],
}

/// `ECEC`: an entity-control component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityControl {
    /// `+0x00`, type `0x006`.
    pub animation: u64,
    pub unknown_08: u64,
    /// `+0x10`, type `0x01C`.
    pub identifier: u64,
    pub unknown_18: u64,
    pub unknown_20: u32,
    /// `+0x24..+0x36`, nine `u16`s.
    pub words: [u16; 9],
    pub unknown_36: u16,
}

/// `ECSO`: a sound component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sound {
    /// `+0x00`, type `0x02C`.
    pub sound: u64,
    pub unknown_08: u64,
    pub unknown_10: u16,
    pub unknown_12: u8,
}

/// `ECVS`: a voice-stimulus component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceStimulus {
    /// `+0x00`, type `0x078`.
    pub stimulus: u64,
    pub unknown_08: u32,
    pub unknown_0c: u32,
    pub unknown_10: u32,
    pub unknown_14: u8,
    pub unknown_18: u64,
    pub unknown_20: u64,
}

/// `ECRR`: a ribbon-renderer component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ribbon {
    /// `+0x00`, type `0x00C`.
    pub model: u64,
    pub unknown_08: u32,
    /// `+0x0c..+0x1a`.
    pub head: [u16; 7],
    /// `+0x1c..+0x24`.
    pub middle: [u16; 4],
    /// `+0x26..+0x38`.
    pub tail: [u16; 9],
    pub unknown_38: u8,
}

/// A parsed effect asset.
#[derive(Debug, Clone)]
pub struct Effect<'a> {
    /// The `EFCT` root's payload.
    pub header: Header<'a>,
    /// Every chunk below the root, in document order.
    pub chunks: Vec<Chunk<'a>>,
}

impl<'a> Effect<'a> {
    /// Parse an effect asset.
    ///
    /// The root must be `EFCT` and at least [`ROOT_MIN`] bytes. Every child is
    /// kept, whether or not its tag is in [`KIND_TABLE`]; a tag that is not gets
    /// [`NO_KIND`] rather than being dropped.
    pub fn parse(data: &'a [u8]) -> Result<Effect<'a>> {
        let tree: Chunks = Chunks::parse(data).map_err(Error::NotChunked)?;
        if tree.header.id != ROOT {
            return Err(Error::NotAnEffect(tree.header.id));
        }
        let root = tree
            .entries
            .iter()
            .find(|e| e.tag == ROOT)
            .ok_or(Error::ShortHeader {
                need: ROOT_MIN,
                got: 0,
            })?;
        let header_payload =
            data.get(root.offset..root.offset + root.size)
                .ok_or(Error::ShortHeader {
                    need: ROOT_MIN,
                    got: 0,
                })?;
        if header_payload.len() < ROOT_MIN {
            return Err(Error::ShortHeader {
                need: ROOT_MIN,
                got: header_payload.len(),
            });
        }

        let mut chunks = Vec::new();
        for entry in &tree.entries {
            if entry.tag == ROOT {
                continue;
            }
            let Some(payload) = data.get(entry.offset..entry.offset + entry.size) else {
                continue;
            };
            chunks.push(Chunk {
                tag: entry.tag,
                kind: kind_of(&entry.tag),
                version: entry.version,
                depth: entry.depth as u8,
                payload,
            });
        }

        Ok(Effect {
            header: Header {
                payload: header_payload,
            },
            chunks,
        })
    }

    /// The first chunk with a tag, in document order.
    pub fn chunk(&self, tag: &[u8; 4]) -> Option<&Chunk<'a>> {
        self.chunks.iter().find(|c| &c.tag == tag)
    }

    /// Every chunk with a tag.
    pub fn chunks_with<'s>(&'s self, tag: &'s [u8; 4]) -> impl Iterator<Item = &'s Chunk<'a>> {
        self.chunks.iter().filter(move |c| &c.tag == tag)
    }

    /// Every nested effect (`ECEF`) in document order.
    pub fn nested_effects(&self) -> impl Iterator<Item = NestedEffect> + '_ {
        self.chunks.iter().filter_map(|c| c.effect())
    }

    /// Every model component (`ECMD`) in document order.
    pub fn models(&self) -> impl Iterator<Item = Model> + '_ {
        self.chunks.iter().filter_map(|c| c.model())
    }

    /// The version every asset on the install carries for `tag`, where the
    /// sweep measured one.
    pub fn observed_version(tag: &[u8; 4]) -> Option<u16> {
        OBSERVED_VERSIONS
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| *v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::{HEADER_SIZE, MAGIC, SENTINEL};

    /// Build a chunked asset: `(tag, version, payload)`, flat, in order.
    fn asset(root: &[u8; 4], root_payload: &[u8], children: &[([u8; 4], u16, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&chunks::encode_tag(root).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        let mut emit = |tag: &[u8; 4], version: u16, payload: &[u8]| {
            out.extend_from_slice(&chunks::encode_tag(tag).to_le_bytes());
            out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            out.extend_from_slice(&SENTINEL.to_le_bytes());
            out.extend_from_slice(&version.to_le_bytes());
            out.extend_from_slice(payload);
        };
        emit(root, ROOT_VERSION, root_payload);
        for (tag, version, payload) in children {
            emit(tag, *version, payload);
        }
        let size = (out.len() - HEADER_SIZE) as i32;
        out[8..12].copy_from_slice(&size.to_le_bytes());
        out
    }

    fn header_payload() -> Vec<u8> {
        let mut h = vec![0u8; 0x80];
        h[header::COUNT] = 2;
        h
    }

    #[test]
    fn the_kind_table_is_the_clients_0x34_entries() {
        assert_eq!(KIND_TABLE.len(), 0x34);
        for (i, tag) in KIND_TABLE.iter().enumerate() {
            assert_eq!(
                kind_of(tag),
                i as u8 + 1,
                "slot of {}",
                String::from_utf8_lossy(tag)
            );
            assert_eq!(tag_of(i as u8 + 1), Some(*tag));
        }
        assert_eq!(kind_of(b"ZZZZ"), NO_KIND);
        assert_eq!(tag_of(0), None);
        assert_eq!(tag_of(NO_KIND), None);
        // A tag that is an effect chunk on the install but has no runtime kind.
        assert_eq!(kind_of(b"ECMP"), NO_KIND);
        assert_eq!(kind_of(b"ECSS"), NO_KIND);
        assert_eq!(kind_of(b"ESHT"), NO_KIND);
    }

    #[test]
    fn parse_reads_the_root_and_every_child() {
        let mut model = vec![0u8; 0x20];
        model[0x00..0x08].copy_from_slice(&0x0D00_0000_0000_0001u64.to_le_bytes());
        model[0x08..0x10].copy_from_slice(&0x0980_0000_0000_0002u64.to_le_bytes());
        model[0x10..0x18].copy_from_slice(&0x0A00_0000_0000_0003u64.to_le_bytes());
        model[0x18] = 1;
        model[0x19] = 0;

        let mut sound = vec![0u8; 0x20];
        sound[0x00..0x08].copy_from_slice(&0x0D40_0000_0000_0004u64.to_le_bytes());
        sound[0x10..0x12].copy_from_slice(&7u16.to_le_bytes());
        sound[0x12] = 9;

        let h = header_payload();
        let data = asset(&ROOT, &h, &[(*b"ECMD", 1, &model), (*b"ECSO", 0, &sound)]);
        let e = Effect::parse(&data).unwrap();
        assert_eq!(e.header.array_count(), 2);
        assert_eq!(e.chunks.len(), 2);
        assert_eq!(e.chunks[0].tag, *b"ECMD");
        assert_eq!(e.chunks[0].kind, kind_of(b"ECMD"));
        assert_eq!(e.chunks[0].version, 1);

        let m = e.chunks[0].model().unwrap();
        assert_eq!(m.model, 0x0D00_0000_0000_0001);
        assert_eq!(m.look, 0x0980_0000_0000_0002);
        assert_eq!(m.animation, 0x0A00_0000_0000_0003);
        assert_eq!(m.unknown_18, 1);
        assert_eq!(e.models().count(), 1);

        let s = e.chunks[1].sound().unwrap();
        assert_eq!(s.sound, 0x0D40_0000_0000_0004);
        assert_eq!(s.unknown_10, 7);
        assert_eq!(s.unknown_12, 9);
    }

    #[test]
    fn a_short_component_is_not_read_as_a_component() {
        let h = header_payload();
        let data = asset(&ROOT, &h, &[(*b"ECMD", 1, &[0u8; 0x18])]);
        let e = Effect::parse(&data).unwrap();
        assert!(e.chunks[0].model().is_none());
    }

    #[test]
    fn a_short_root_is_rejected() {
        let data = asset(&ROOT, &[0u8; 0x40], &[]);
        assert!(matches!(
            Effect::parse(&data),
            Err(Error::ShortHeader { .. })
        ));
    }

    #[test]
    fn the_root_must_be_efct() {
        let h = header_payload();
        let data = asset(b"MODL", &h, &[]);
        assert!(matches!(Effect::parse(&data), Err(Error::NotAnEffect(_))));
    }

    #[test]
    fn an_unknown_tag_is_kept_with_no_kind() {
        let h = header_payload();
        let data = asset(&ROOT, &h, &[(*b"ZZZZ", 3, &[1, 2, 3, 4])]);
        let e = Effect::parse(&data).unwrap();
        assert_eq!(e.chunks.len(), 1);
        assert_eq!(e.chunks[0].kind, NO_KIND);
        assert_eq!(e.chunks[0].payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn entity_control_reads_its_words() {
        let mut p = vec![0u8; 0x40];
        p[0x00..0x08].copy_from_slice(&0x0A00_0000_0000_0001u64.to_le_bytes());
        p[0x10..0x18].copy_from_slice(&0x0D80_0000_0000_0002u64.to_le_bytes());
        for i in 0..9u16 {
            let at = 0x24 + i as usize * 2;
            p[at..at + 2].copy_from_slice(&(0x100 + i).to_le_bytes());
        }
        p[0x36..0x38].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let h = header_payload();
        let data = asset(&ROOT, &h, &[(*b"ECEC", 0, &p)]);
        let e = Effect::parse(&data).unwrap();
        let c = e.chunks[0].entity_control().unwrap();
        assert_eq!(c.animation, 0x0A00_0000_0000_0001);
        assert_eq!(c.identifier, 0x0D80_0000_0000_0002);
        assert_eq!(c.words[0], 0x100);
        assert_eq!(c.words[8], 0x108);
        assert_eq!(c.unknown_36, 0xFFFF);
    }

    #[test]
    fn the_builder_reads_the_root_header_fields_it_is_documented_to() {
        // The prologue's last read is the byte at +0x65; +0x66 is the count.
        let mut h = vec![0u8; 0x80];
        h[header::COUNT] = 0x11;
        h[0x65] = 0x22;
        let data = asset(&ROOT, &h, &[]);
        let e = Effect::parse(&data).unwrap();
        assert_eq!(e.header.array_count(), 0x11);
        assert_eq!(e.header.payload[0x65], 0x22);
    }

    #[test]
    fn the_measured_versions_are_unique_and_readable() {
        for (i, (tag, version)) in OBSERVED_VERSIONS.iter().enumerate() {
            assert!(
                OBSERVED_VERSIONS[i + 1..].iter().all(|(t, _)| t != tag),
                "{} appears twice",
                String::from_utf8_lossy(&tag[..])
            );
            assert_eq!(Effect::observed_version(tag), Some(*version));
        }
        // The root's version is also a constant.
        assert_eq!(Effect::observed_version(&ROOT), Some(ROOT_VERSION));
        assert_eq!(Effect::observed_version(b"ZZZZ"), None);
    }
}

//! The map chunk: how a map's contents are laid out in the file.
//!
//! Not a chunked-data container. It is a header and a run of *placeables*, each
//! of which says what to put where:
//!
//! ```text
//! header        u32 placeable count, u32 instance-data offset,
//!               u32 placeable offset, u32 unknown
//! placeable     UUID (16), three flag bytes, u32 size, then the body
//! ```
//!
//! `size` covers the whole placeable including its header, so a placeable whose
//! body is not understood can still be skipped exactly -- which is what makes it
//! safe to read a map whose full vocabulary of placeable kinds is not known.
//!
//! The *kind* of body is not in the placeable. A map chunk asset carries one
//! kind throughout, chosen by the game from the asset's role, so the caller says
//! which kind it is reading. Where that is not known, [`Kind::detect`] tries
//! the kinds it can score and keeps the one whose asset references resolve. The
//! reference library locates a map's chunk by *building* the asset key from the
//! map header with the kind in bits 32..39 (`InstanceExtensions.GetChunkKey`),
//! so a caller that has the map header does not have to guess.
//!
//! Two evidence classes meet in this file:
//!
//! * The **model and entity** bodies are read at the offsets real files use --
//!   pinned by asset scans recorded in `docs/formats/materials.md` -- and the
//!   model layout deliberately differs from the reference's generated struct
//!   (see [`Kind::Model`]).
//! * The remaining bodies (**model group, single model, light, reflection
//!   point, text, sound, effect, sequence, area**) are ported from the
//!   reference reader `TankLib/teMapChunk.cs` (the public C# Overwatch asset
//!   library), field offset for field offset. They are **not verified against a
//!   real chunk of their kind** here; the sweep in `tests/mapchunk_kinds.rs`
//!   measures how they fare on the install rather than asserting they are
//!   right.

use std::fmt;

use crate::guid;

pub const HEADER_SIZE: usize = 16;
pub const COMMON_SIZE: usize = 24;

/// Body size of a model placeable as the real files lay it out. The reference's
/// generated struct is shorter (0x48); see [`Kind::Model`].
pub const MODEL_BODY_SIZE: usize = 0x68;

/// Body-relative size of the reference's `teMapPlaceableModelGroup.Structure`.
pub const MODEL_GROUP_HEADER_SIZE: usize = 0x20;
/// Size of one `teMapPlaceableModelGroup.Group`.
pub const MODEL_GROUP_GROUP_SIZE: usize = 16;
/// Size of one `teMapPlaceableModelGroup.Entry`.
pub const MODEL_GROUP_ENTRY_SIZE: usize = 96;

/// Body-relative offset at which an entity's instance-data record table starts
/// (the reference's `const int instArrayOffset = 104`).
pub const ENTITY_INSTANCE_TABLE_OFFSET: usize = 104;
/// Size of one instance-data record: `u32 type`, `u32 relative offset`.
pub const ENTITY_INSTANCE_RECORD_SIZE: usize = 8;

/// Body size of the reference's `teMapPlaceableLight.Structure`.
pub const LIGHT_BODY_SIZE: usize = 184;
/// Body size of the reference's `teMapPlaceableReflectionPoint.Structure`.
pub const REFLECTION_POINT_BODY_SIZE: usize = 168;
/// Body size of the reference's `teMapPlaceableText.Structure`.
pub const TEXT_BODY_SIZE: usize = 56;
/// Body size of the reference's `teMapPlaceableSound.Structure`.
pub const SOUND_BODY_SIZE: usize = 48;
/// Body size of the reference's `teMapPlaceableEffect.Structure` (also used by
/// the sequence placeable).
pub const EFFECT_BODY_SIZE: usize = 48;
/// Body size of the reference's `teMapPlaceableArea.Structure`.
pub const AREA_STRUCT_SIZE: usize = 44;
/// Size of one `teMapPlaceableArea.Box`.
pub const AREA_BOX_SIZE: usize = 56;

/// What a map chunk's placeables describe.
///
/// The ids are the reference's `teMAP_PLACEABLE_TYPE` values (`Enums.cs`); the
/// client builds a map's chunk key with this id in bits 32..39 of the asset
/// GUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Static geometry: a model and its placement.
    Model,
    /// A model placed on its own. The reference gives it the same body struct
    /// as [`Kind::Model`] (`teMapPlaceableSingleModel` reuses
    /// `teMapPlaceableModel.Structure`).
    SingleModel,
    /// A group of models placed together: a group array, each group carrying a
    /// look and an entry array.
    ModelGroup,
    /// Gameplay things: entities with component data attached.
    Entity,
    /// A light: type, colour, cone angle, projection textures.
    Light,
    /// A reflection probe: two textures plus identity and corner frames.
    ReflectionPoint,
    /// A text placement: a string asset, a font, a material.
    Text,
    /// A sound emitter: a sound asset and its transform.
    Sound,
    /// An effect emitter: an effect asset and its transform.
    Effect,
    /// A sequence: the reference reuses the effect structure.
    Sequence,
    /// A trigger volume: box/sphere/capsule counts and offsets.
    Area,
    /// A kind this reader has no body for. The reference's
    /// `teMapPlaceableDummy` reads no fields and skips the placeable by its
    /// declared size, which is all [`MapChunk::parse`] does here too.
    Unknown,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::SingleModel => "single model",
            Kind::ModelGroup => "model group",
            Kind::Entity => "entity",
            Kind::Light => "light",
            Kind::ReflectionPoint => "reflection point",
            Kind::Text => "text",
            Kind::Sound => "sound",
            Kind::Effect => "effect",
            Kind::Sequence => "sequence",
            Kind::Area => "area",
            Kind::Unknown => "unknown",
        }
    }

    /// The kind's id in the reference's `teMAP_PLACEABLE_TYPE` enum.
    pub fn type_id(self) -> u8 {
        match self {
            Kind::ModelGroup => 0x01,
            Kind::SingleModel => 0x02,
            Kind::ReflectionPoint => 0x04,
            Kind::Text => 0x07,
            Kind::Model => 0x08,
            Kind::Light => 0x09,
            Kind::Area => 0x0A,
            Kind::Entity => 0x0B,
            Kind::Sound => 0x0C,
            Kind::Effect => 0x0D,
            Kind::Sequence => 0x18,
            Kind::Unknown => 0x00,
        }
    }

    /// The inverse of [`Kind::type_id`]. `None` for ids the reference's enum
    /// names but this reader has no body for (occluder, clutter, label, fog,
    /// indoor box, post-processing, planar reflection surface).
    pub fn from_type_id(id: u8) -> Option<Kind> {
        Some(match id {
            0x00 => Kind::Unknown,
            0x01 => Kind::ModelGroup,
            0x02 => Kind::SingleModel,
            0x04 => Kind::ReflectionPoint,
            0x07 => Kind::Text,
            0x08 => Kind::Model,
            0x09 => Kind::Light,
            0x0A => Kind::Area,
            0x0B => Kind::Entity,
            0x0C => Kind::Sound,
            0x0D => Kind::Effect,
            0x18 => Kind::Sequence,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed map chunk: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub placeable_count: u32,
    /// Offset of the component instance data, 16 bytes before it is usable
    /// (the reference seeks to `InstanceDataOffset + 16` before walking entity
    /// instance data). `0` means the reference does not read any.
    pub instance_data_offset: u32,
    pub placeable_offset: u32,
    pub unknown: u32,
}

impl Header {
    /// Where the component-instance-data region starts, per the reference's
    /// `InstanceDataOffset + 16` rule. `None` when the field is zero.
    pub fn instance_data_at(&self) -> Option<usize> {
        (self.instance_data_offset != 0).then(|| self.instance_data_offset as usize + 16)
    }
}

/// Kind-specific body data. Which variant a placeable carries follows the
/// [`Kind`] the chunk was parsed with.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableModel`. The model, the
    /// look and the transform live in [`Placeable`]'s flat fields; the body
    /// carries the reference's trailing `Unknown` quaternion.
    Model(ModelBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableSingleModel`, which
    /// reuses the model structure. Same fields and offsets as [`Body::Model`].
    SingleModel(ModelBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableModelGroup`.
    ModelGroup(ModelGroupBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableEntity`.
    Entity(EntityBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableLight`.
    Light(LightBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableReflectionPoint`.
    ReflectionPoint(ReflectionPointBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableText`.
    Text(TextBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableSound`.
    Sound(SoundBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableEffect`.
    Effect(EffectBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableSequence`, which
    /// reuses the effect structure.
    Sequence(EffectBody),
    /// Reference reader `teMapChunk.cs`, `teMapPlaceableArea`.
    Area(AreaBody),
    /// No body reader at all (the reference's `teMapPlaceableDummy`).
    Unknown,
}

/// The trailing `teQuat Unknown` of a model placeable.
///
/// The reference defines the field but not its meaning; it is at `body + 0x58`
/// in the real files (the reference struct would put it at `+0x38`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelBody {
    pub unknown_rotation: [f32; 4],
}

/// The body of a model group placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableModelGroup`: a
/// [`MODEL_GROUP_HEADER_SIZE`]-byte structure, then `group_count` groups of
/// [`MODEL_GROUP_GROUP_SIZE`] bytes, each followed by its `entry_count`
/// [`MODEL_GROUP_ENTRY_SIZE`]-byte entries.
///
/// Everything here is read from the placeable's declared `size`: an array that
/// does not fit is not followed into the next placeable, and the collected
/// lengths say so.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelGroupBody {
    /// `Structure.Model`: the model every entry instantiates. Mirrored in
    /// [`Placeable::model`].
    pub model: Option<u64>,
    /// `Structure.GroupCount`, as declared (may be larger than `groups.len()`
    /// when the declared size does not hold that many).
    pub group_count: u32,
    /// `Structure.TotalModelCount`.
    pub total_model_count: u32,
    /// `Structure.Mask`, meaning unknown.
    pub mask: u32,
    /// `Structure.Unk1..Unk3`, meaning unknown.
    pub unknown: [u32; 3],
    /// The groups that fit in the placeable.
    pub groups: Vec<ModelGroup>,
}

/// One `teMapPlaceableModelGroup.Group`, followed by its entries.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelGroup {
    /// `Group.ModelLook`.
    pub look: Option<u64>,
    /// `Group.UnkSize`, meaning unknown.
    pub unknown_size: u32,
    /// `Group.EntryCount`, as declared (may exceed `entries.len()`).
    pub entry_count: u32,
    /// The entries that fit in the placeable.
    pub entries: Vec<ModelGroupEntry>,
}

/// One `teMapPlaceableModelGroup.Entry`: a full transform per model in the
/// group, then unknown words.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelGroupEntry {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// Rotation as a quaternion, x/y/z/w.
    pub rotation: [f32; 4],
    /// `Entry.Unk1..Unk7` at entry `+0x28`, meaning unknown.
    pub unknown_a: [u32; 7],
    /// `Entry.Unk8..UnkE` at entry `+0x44`, meaning unknown.
    pub unknown_b: [i32; 7],
}

/// The body of an entity placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableEntity`: the entity and
/// identifier references and the transform are in [`Placeable`]'s flat fields;
/// this carries the component-instance-data *records*, a table of
/// `instance_data_count` [`InstanceDataRef`]s at `body + 104`.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityBody {
    /// The records that fit inside the placeable's declared size, in order.
    pub instance_data: Vec<InstanceDataRef>,
}

/// One component-instance-data record of an entity placeable.
///
/// The reference reads two `u32`s per record: a type it discards
/// (`var _ = reader.ReadUInt32(); // type`) and an offset it resolves as
/// `record_start + relative_offset`, an absolute offset within the asset.
///
/// What is exposed here is exactly that: the type word (likely the component's
/// STU instance hash, e.g. `STUComponentInstanceData` = `0xA46203AB` in the
/// reference's type table -- **not verified** in this reader) and the target
/// offset. The reference then parses a whole STU stream at the target; this
/// reader does not, so the data *length* under the record is unknown here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceDataRef {
    /// The record's first word, discarded by the reference.
    pub type_hash: u32,
    /// `record_start + relative_offset`: where the component's STU stream is.
    pub offset: usize,
}

/// The body of a light placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableLight`, at these
/// body-relative offsets: rotation `+0x00`, translation `+0x10`, two unknown
/// words `+0x1C`/`+0x20`, type `+0x24`, two more unknown words `+0x28`/`+0x2C`,
/// colour `+0x30` (three `f32`), unknown position/quaternion pairs from `+0x3C`,
/// cone angle `+0x58`, more unknown frames, intensity guess `+0x90`, five
/// unknown floats `+0x94`, projection textures `+0xA8`/`+0xB0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightBody {
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    /// `Unknown1`.
    pub unknown_1: u32,
    /// `Unknown2`.
    pub unknown_2: u32,
    /// `teLIGHTTYPE`: 0 point, 1 frustum, 2 none (reference enum).
    pub light_type: u32,
    /// `Unknown3`.
    pub unknown_3: u32,
    /// `Unknown4`.
    pub unknown_4: u32,
    /// `teColorRGB` -- three floats in the reference, not bytes.
    pub color: [f32; 3],
    pub unknown_pos_1: [f32; 3],
    pub unknown_quat_1: [f32; 4],
    /// Cone angle in degrees; `-1.0` for point lights (reference comment).
    pub light_fov: f32,
    pub unknown_pos_2: [f32; 3],
    pub unknown_quat_2: [f32; 4],
    pub unknown_pos_3: [f32; 3],
    pub unknown_pos_4: [f32; 3],
    /// The reference names this `IntensityGUESS`: its meaning is a guess.
    pub intensity: f32,
    /// `Unknown6..Unknown10`.
    pub unknown_floats: [f32; 5],
    pub projection_texture_1: Option<u64>,
    pub projection_texture_2: Option<u64>,
}

/// The body of a reflection point placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableReflectionPoint`, at these
/// body-relative offsets: translation `+0x00`, scale `+0x0C`, rotation `+0x18`,
/// identity translation/scale/rotation `+0x28`/`+0x34`/`+0x40`, corner `+0x50`,
/// unknown `+0x5C`, textures `+0x60`/`+0x68`, unknowns `+0x70`/`+0x74`, seven
/// floats `+0x78`, five ints `+0x94`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReflectionPointBody {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
    pub identity_translation: [f32; 3],
    pub identity_scale: [f32; 3],
    pub identity_rotation: [f32; 4],
    pub corner: [f32; 3],
    pub unknown_1: u32,
    pub texture_1: Option<u64>,
    pub texture_2: Option<u64>,
    pub unknown_2: u32,
    pub unknown_3: u32,
    pub unknown_4: [f32; 7],
    pub unknown_5: [i32; 5],
}

/// The body of a text placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableText`: eight floats at
/// `+0x00` whose meaning is not recovered, then three references at `+0x20`,
/// `+0x28` and `+0x30`. The reference names them `String`, `MapFont` and
/// `Material`; the text itself lives in the string asset, so this body carries
/// the reference, not characters.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBody {
    /// `FloatA..FloatH`, meaning unknown.
    pub floats: [f32; 8],
    /// The text's string asset.
    pub string_guid: Option<u64>,
    pub map_font: Option<u64>,
    pub material: Option<u64>,
}

/// The body of a sound placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableSound`: the sound asset at
/// `+0x00`, then translation `+0x08`, scale `+0x14`, rotation `+0x20`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundBody {
    pub sound: Option<u64>,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
}

/// The body of an effect (or sequence) placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableEffect` (and
/// `teMapPlaceableSequence`, which reuses it): the effect asset at `+0x00`,
/// then translation `+0x08`, scale `+0x14`, rotation `+0x20`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectBody {
    pub effect: Option<u64>,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
}

/// The body of an area (trigger volume) placeable.
///
/// Reference reader `teMapChunk.cs`, `teMapPlaceableArea`, an explicit-layout
/// structure: five `u16` counts at `+0x0C..+0x16`, then five `u32` offsets at
/// `+0x18..+0x2C`, all relative to the body. Only the box array's element
/// layout is defined by the reference; the sphere, capsule and two unknown
/// arrays are counted and located here but not decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaBody {
    pub box_count: u16,
    pub sphere_count: u16,
    pub capsule_count: u16,
    pub unknown_1_count: u16,
    pub unknown_2_count: u16,
    pub box_offset: u32,
    pub sphere_offset: u32,
    pub capsule_offset: u32,
    pub unknown_1_offset: u32,
    pub unknown_2_offset: u32,
    /// The boxes that fit in the placeable, in order. May be fewer than
    /// `box_count` when the declared offset/count runs past the declared size.
    pub boxes: Vec<AreaBox>,
}

/// One `teMapPlaceableArea.Box`: an oriented box, 56 bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaBox {
    /// `Box.Orientation` (`System.Numerics.Quaternion`, x/y/z/w).
    pub orientation: [f32; 4],
    pub translation: [f32; 3],
    pub extents: [f32; 3],
    /// `Box.NoIdea`: a `Vector4` the reference does not name.
    pub unknown: [f32; 4],
}

/// A placement: what to put, and where.
#[derive(Debug, Clone)]
pub struct Placeable {
    pub uuid: [u8; 16],
    /// Index of this placeable within the chunk.
    pub index: usize,
    /// Byte offset of the placeable within the asset.
    pub offset: usize,
    /// Total length including its own header.
    pub size: usize,
    /// Model asset, for the kinds that name one.
    pub model: Option<u64>,
    /// Second asset reference: the model's look/variant.
    pub look: Option<u64>,
    /// Entity definition asset, for entity placeables.
    pub entity: Option<u64>,
    /// Identifier token asset, for entity placeables.
    pub identifier: Option<u64>,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// Rotation as a quaternion, x/y/z/w.
    pub rotation: [f32; 4],
    /// Number of component data blobs attached to an entity.
    pub instance_data_count: u32,
    /// The kind-specific body, as the [`Kind`] the chunk was parsed with.
    pub body: Body,
}

#[derive(Debug, Clone)]
pub struct MapChunk {
    pub header: Header,
    pub placeables: Vec<Placeable>,
}

fn rd_u16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn rd_u32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd_f32(data: &[u8], at: usize) -> Option<f32> {
    rd_u32(data, at).map(f32::from_bits)
}

/// A 64-bit asset reference stored as two little-endian `u32`s (the low half
/// first, matching how the files lay the reference out).
fn rd_guid(data: &[u8], at: usize) -> Option<u64> {
    rd_u32(data, at)
        .zip(rd_u32(data, at + 4))
        .map(|(lo, hi)| (lo as u64) | ((hi as u64) << 32))
}

fn rd_i32(data: &[u8], at: usize) -> Option<i32> {
    rd_u32(data, at).map(|v| v as i32)
}

fn rd_vec3(data: &[u8], at: usize) -> Option<[f32; 3]> {
    Some([
        rd_f32(data, at)?,
        rd_f32(data, at + 4)?,
        rd_f32(data, at + 8)?,
    ])
}

fn rd_quat(data: &[u8], at: usize) -> Option<[f32; 4]> {
    Some([
        rd_f32(data, at)?,
        rd_f32(data, at + 4)?,
        rd_f32(data, at + 8)?,
        rd_f32(data, at + 12)?,
    ])
}

impl MapChunk {
    pub fn parse(data: &[u8], kind: Kind) -> Result<MapChunk> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Malformed("shorter than its header"));
        }
        let header = Header {
            placeable_count: rd_u32(data, 0).ok_or(Error::Malformed("truncated header"))?,
            instance_data_offset: rd_u32(data, 4).ok_or(Error::Malformed("truncated header"))?,
            placeable_offset: rd_u32(data, 8).ok_or(Error::Malformed("truncated header"))?,
            unknown: rd_u32(data, 12).ok_or(Error::Malformed("truncated header"))?,
        };

        let mut placeables = Vec::new();
        let mut at = header.placeable_offset as usize;
        for index in 0..header.placeable_count as usize {
            if at + COMMON_SIZE > data.len() {
                break;
            }
            let size =
                rd_u32(data, at + 20).ok_or(Error::Malformed("truncated placeable"))? as usize;
            if size < COMMON_SIZE || at + size > data.len() {
                break;
            }
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&data[at..at + 16]);

            placeables.push(read_placeable(data, kind, uuid, index, at, size));
            at += size;
        }

        Ok(MapChunk { header, placeables })
    }

    /// Pick the kind whose references look like real assets.
    ///
    /// Only [`Kind::Model`] and [`Kind::Entity`] are tried -- the two kinds
    /// whose first body words are asset references and whose layout has been
    /// measured on real chunks. A caller that knows the kind (for example from
    /// the map header, see the module docs) declares it and calls
    /// [`MapChunk::parse`] directly.
    ///
    /// `resolves` is asked about each candidate reference. The wrong kind reads
    /// a transform as a GUID, which almost never lands on an asset that exists,
    /// so the right kind is the one that keeps its references.
    pub fn detect(data: &[u8], resolves: &dyn Fn(u64) -> bool) -> Result<(Kind, MapChunk)> {
        let mut best: Option<(Kind, MapChunk, usize)> = None;
        for kind in [Kind::Model, Kind::Entity] {
            let Ok(chunk) = MapChunk::parse(data, kind) else {
                continue;
            };
            let hits = chunk
                .placeables
                .iter()
                .filter(|p| p.model.or(p.entity).map(resolves).unwrap_or(false))
                .count();
            if best.as_ref().map(|(_, _, b)| hits > *b).unwrap_or(true) {
                best = Some((kind, chunk, hits));
            }
        }
        match best {
            Some((kind, chunk, _)) => Ok((kind, chunk)),
            None => Err(Error::Malformed("no placeable kind fits")),
        }
    }

    /// How many placeables name a model that resolves.
    pub fn resolvable_models(&self, resolves: &dyn Fn(u64) -> bool) -> usize {
        self.placeables
            .iter()
            .filter(|p| p.model.map(resolves).unwrap_or(false))
            .count()
    }

    pub fn summary(&self) -> String {
        let models = self.placeables.iter().filter(|p| p.model.is_some()).count();
        let entities = self
            .placeables
            .iter()
            .filter(|p| p.entity.is_some())
            .count();
        format!(
            "{} placeables ({models} with a model, {entities} with an entity)",
            self.placeables.len()
        )
    }
}

/// Read one placeable's body under a declared kind.
///
/// Reads are lenient about a placeable shorter than its structure (fields that
/// do not fit stay at their defaults, as before this reader grew kinds), but
/// *arrays* (model-group groups/entries, entity records, area boxes) are
/// bounded by the placeable's declared `size`: the reference's manager seeks to
/// the declared end after reading, so an array that does not fit would run into
/// the next placeable and cannot be the layout being read.
fn read_placeable(
    data: &[u8],
    kind: Kind,
    uuid: [u8; 16],
    index: usize,
    offset: usize,
    size: usize,
) -> Placeable {
    let body = offset + COMMON_SIZE;
    let body_end = offset + size;
    let mut placeable = Placeable {
        uuid,
        index,
        offset,
        size,
        model: None,
        look: None,
        entity: None,
        identifier: None,
        translation: [0.0; 3],
        scale: [1.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        instance_data_count: 0,
        body: Body::Unknown,
    };

    match kind {
        // The model reference comes first, then a run of counters, then the
        // placement. The generated type for this structure puts the transform
        // straight after the second reference, which reads zeroes and huge
        // numbers -- the file was followed instead: on 16,357 model placeables
        // exactly +0x00 (the model, type 0x00C) and +0x20 (the look, type
        // 0x01A) resolve as assets (`docs/formats/materials.md`).
        //
        // `teMapPlaceableSingleModel` reuses the model structure in the
        // reference, so it is read the same way here.
        Kind::Model | Kind::SingleModel => {
            placeable.model = rd_guid(data, body);
            placeable.look = rd_guid(data, body + 0x20);
            placeable.translation = rd_vec3(data, body + 0x30).unwrap_or([0.0; 3]);
            placeable.scale = rd_vec3(data, body + 0x3C).unwrap_or([1.0; 3]);
            placeable.rotation = rd_quat(data, body + 0x48).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let model = ModelBody {
                unknown_rotation: rd_quat(data, body + 0x58).unwrap_or([0.0; 4]),
            };
            placeable.body = if kind == Kind::Model {
                Body::Model(model)
            } else {
                Body::SingleModel(model)
            };
        }
        // Reference reader teMapChunk.cs, teMapPlaceableModelGroup.
        Kind::ModelGroup => {
            let model = rd_guid(data, body);
            placeable.model = model;
            let group_count = rd_u32(data, body + 8).unwrap_or(0);
            let total_model_count = rd_u32(data, body + 0x0C).unwrap_or(0);
            let mask = rd_u32(data, body + 0x10).unwrap_or(0);
            let unknown = [
                rd_u32(data, body + 0x14).unwrap_or(0),
                rd_u32(data, body + 0x18).unwrap_or(0),
                rd_u32(data, body + 0x1C).unwrap_or(0),
            ];

            let mut groups = Vec::new();
            let mut at = body + MODEL_GROUP_HEADER_SIZE;
            for _ in 0..group_count {
                if at + MODEL_GROUP_GROUP_SIZE > body_end {
                    break;
                }
                let look = rd_guid(data, at);
                let unknown_size = rd_u32(data, at + 8).unwrap_or(0);
                let entry_count = rd_u32(data, at + 12).unwrap_or(0);
                at += MODEL_GROUP_GROUP_SIZE;

                let mut entries = Vec::new();
                let mut truncated = false;
                for _ in 0..entry_count {
                    if at + MODEL_GROUP_ENTRY_SIZE > body_end {
                        truncated = true;
                        break;
                    }
                    entries.push(ModelGroupEntry {
                        translation: rd_vec3(data, at).unwrap_or([0.0; 3]),
                        scale: rd_vec3(data, at + 12).unwrap_or([1.0; 3]),
                        rotation: rd_quat(data, at + 24).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                        unknown_a: [
                            rd_u32(data, at + 40).unwrap_or(0),
                            rd_u32(data, at + 44).unwrap_or(0),
                            rd_u32(data, at + 48).unwrap_or(0),
                            rd_u32(data, at + 52).unwrap_or(0),
                            rd_u32(data, at + 56).unwrap_or(0),
                            rd_u32(data, at + 60).unwrap_or(0),
                            rd_u32(data, at + 64).unwrap_or(0),
                        ],
                        unknown_b: [
                            rd_i32(data, at + 68).unwrap_or(0),
                            rd_i32(data, at + 72).unwrap_or(0),
                            rd_i32(data, at + 76).unwrap_or(0),
                            rd_i32(data, at + 80).unwrap_or(0),
                            rd_i32(data, at + 84).unwrap_or(0),
                            rd_i32(data, at + 88).unwrap_or(0),
                            rd_i32(data, at + 92).unwrap_or(0),
                        ],
                    });
                    at += MODEL_GROUP_ENTRY_SIZE;
                }

                groups.push(ModelGroup {
                    look,
                    unknown_size,
                    entry_count,
                    entries,
                });
                // A partial entry means the next group's header cannot be
                // located either, so stop the whole walk rather than guess.
                if truncated {
                    break;
                }
            }

            placeable.body = Body::ModelGroup(ModelGroupBody {
                model,
                group_count,
                total_model_count,
                mask,
                unknown,
                groups,
            });
        }
        // Reference reader teMapChunk.cs, teMapPlaceableEntity: entity
        // definition, identifier, then translation, scale, rotation, instance
        // data count; the record table is at body + 104.
        Kind::Entity => {
            placeable.entity = rd_guid(data, body);
            placeable.identifier = rd_guid(data, body + 8);
            placeable.translation = rd_vec3(data, body + 16).unwrap_or([0.0; 3]);
            placeable.scale = rd_vec3(data, body + 28).unwrap_or([1.0; 3]);
            placeable.rotation = rd_quat(data, body + 40).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let count = rd_u32(data, body + 56).unwrap_or(0);
            placeable.instance_data_count = count;

            let mut instance_data = Vec::new();
            let mut at = body + ENTITY_INSTANCE_TABLE_OFFSET;
            for _ in 0..count {
                if at + ENTITY_INSTANCE_RECORD_SIZE > body_end {
                    break;
                }
                let type_hash = rd_u32(data, at).unwrap_or(0);
                let relative = rd_u32(data, at + 4).unwrap_or(0);
                instance_data.push(InstanceDataRef {
                    type_hash,
                    offset: at + relative as usize,
                });
                at += ENTITY_INSTANCE_RECORD_SIZE;
            }

            placeable.body = Body::Entity(EntityBody { instance_data });
        }
        // Reference reader teMapChunk.cs, teMapPlaceableLight.
        Kind::Light => {
            let rotation = rd_quat(data, body).unwrap_or([0.0; 4]);
            let translation = rd_vec3(data, body + 16).unwrap_or([0.0; 3]);
            placeable.rotation = rotation;
            placeable.translation = translation;
            placeable.body = Body::Light(LightBody {
                rotation,
                translation,
                unknown_1: rd_u32(data, body + 28).unwrap_or(0),
                unknown_2: rd_u32(data, body + 32).unwrap_or(0),
                light_type: rd_u32(data, body + 36).unwrap_or(0),
                unknown_3: rd_u32(data, body + 40).unwrap_or(0),
                unknown_4: rd_u32(data, body + 44).unwrap_or(0),
                color: rd_vec3(data, body + 48).unwrap_or([0.0; 3]),
                unknown_pos_1: rd_vec3(data, body + 60).unwrap_or([0.0; 3]),
                unknown_quat_1: rd_quat(data, body + 72).unwrap_or([0.0; 4]),
                light_fov: rd_f32(data, body + 88).unwrap_or(0.0),
                unknown_pos_2: rd_vec3(data, body + 92).unwrap_or([0.0; 3]),
                unknown_quat_2: rd_quat(data, body + 104).unwrap_or([0.0; 4]),
                unknown_pos_3: rd_vec3(data, body + 120).unwrap_or([0.0; 3]),
                unknown_pos_4: rd_vec3(data, body + 132).unwrap_or([0.0; 3]),
                intensity: rd_f32(data, body + 144).unwrap_or(0.0),
                unknown_floats: [
                    rd_f32(data, body + 148).unwrap_or(0.0),
                    rd_f32(data, body + 152).unwrap_or(0.0),
                    rd_f32(data, body + 156).unwrap_or(0.0),
                    rd_f32(data, body + 160).unwrap_or(0.0),
                    rd_f32(data, body + 164).unwrap_or(0.0),
                ],
                projection_texture_1: rd_guid(data, body + 168),
                projection_texture_2: rd_guid(data, body + 176),
            });
        }
        // Reference reader teMapChunk.cs, teMapPlaceableReflectionPoint.
        Kind::ReflectionPoint => {
            let translation = rd_vec3(data, body).unwrap_or([0.0; 3]);
            let scale = rd_vec3(data, body + 12).unwrap_or([1.0; 3]);
            let rotation = rd_quat(data, body + 24).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            placeable.translation = translation;
            placeable.scale = scale;
            placeable.rotation = rotation;
            placeable.body = Body::ReflectionPoint(ReflectionPointBody {
                translation,
                scale,
                rotation,
                identity_translation: rd_vec3(data, body + 40).unwrap_or([0.0; 3]),
                identity_scale: rd_vec3(data, body + 52).unwrap_or([1.0; 3]),
                identity_rotation: rd_quat(data, body + 64).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                corner: rd_vec3(data, body + 80).unwrap_or([0.0; 3]),
                unknown_1: rd_u32(data, body + 92).unwrap_or(0),
                texture_1: rd_guid(data, body + 96),
                texture_2: rd_guid(data, body + 104),
                unknown_2: rd_u32(data, body + 112).unwrap_or(0),
                unknown_3: rd_u32(data, body + 116).unwrap_or(0),
                unknown_4: [
                    rd_f32(data, body + 120).unwrap_or(0.0),
                    rd_f32(data, body + 124).unwrap_or(0.0),
                    rd_f32(data, body + 128).unwrap_or(0.0),
                    rd_f32(data, body + 132).unwrap_or(0.0),
                    rd_f32(data, body + 136).unwrap_or(0.0),
                    rd_f32(data, body + 140).unwrap_or(0.0),
                    rd_f32(data, body + 144).unwrap_or(0.0),
                ],
                unknown_5: [
                    rd_i32(data, body + 148).unwrap_or(0),
                    rd_i32(data, body + 152).unwrap_or(0),
                    rd_i32(data, body + 156).unwrap_or(0),
                    rd_i32(data, body + 160).unwrap_or(0),
                    rd_i32(data, body + 164).unwrap_or(0),
                ],
            });
        }
        // Reference reader teMapChunk.cs, teMapPlaceableText. There is no
        // transform in the structure; the eight floats' meaning is unknown.
        Kind::Text => {
            placeable.body = Body::Text(TextBody {
                floats: [
                    rd_f32(data, body).unwrap_or(0.0),
                    rd_f32(data, body + 4).unwrap_or(0.0),
                    rd_f32(data, body + 8).unwrap_or(0.0),
                    rd_f32(data, body + 12).unwrap_or(0.0),
                    rd_f32(data, body + 16).unwrap_or(0.0),
                    rd_f32(data, body + 20).unwrap_or(0.0),
                    rd_f32(data, body + 24).unwrap_or(0.0),
                    rd_f32(data, body + 28).unwrap_or(0.0),
                ],
                string_guid: rd_guid(data, body + 32),
                map_font: rd_guid(data, body + 40),
                material: rd_guid(data, body + 48),
            });
        }
        // Reference reader teMapChunk.cs, teMapPlaceableSound.
        Kind::Sound => {
            let translation = rd_vec3(data, body + 8).unwrap_or([0.0; 3]);
            let scale = rd_vec3(data, body + 20).unwrap_or([1.0; 3]);
            let rotation = rd_quat(data, body + 32).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            placeable.translation = translation;
            placeable.scale = scale;
            placeable.rotation = rotation;
            placeable.body = Body::Sound(SoundBody {
                sound: rd_guid(data, body),
                translation,
                scale,
                rotation,
            });
        }
        // Reference reader teMapChunk.cs, teMapPlaceableEffect and
        // teMapPlaceableSequence (which reuses the effect structure).
        Kind::Effect | Kind::Sequence => {
            let translation = rd_vec3(data, body + 8).unwrap_or([0.0; 3]);
            let scale = rd_vec3(data, body + 20).unwrap_or([1.0; 3]);
            let rotation = rd_quat(data, body + 32).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            placeable.translation = translation;
            placeable.scale = scale;
            placeable.rotation = rotation;
            let effect = EffectBody {
                effect: rd_guid(data, body),
                translation,
                scale,
                rotation,
            };
            placeable.body = if kind == Kind::Effect {
                Body::Effect(effect)
            } else {
                Body::Sequence(effect)
            };
        }
        // Reference reader teMapChunk.cs, teMapPlaceableArea. The reference
        // reads only the box array; the other four arrays are counted and
        // located but not decoded here either.
        Kind::Area => {
            let box_count = rd_u16(data, body + 12).unwrap_or(0);
            let sphere_count = rd_u16(data, body + 14).unwrap_or(0);
            let capsule_count = rd_u16(data, body + 16).unwrap_or(0);
            let unknown_1_count = rd_u16(data, body + 18).unwrap_or(0);
            let unknown_2_count = rd_u16(data, body + 20).unwrap_or(0);
            let box_offset = rd_u32(data, body + 24).unwrap_or(0);
            let sphere_offset = rd_u32(data, body + 28).unwrap_or(0);
            let capsule_offset = rd_u32(data, body + 32).unwrap_or(0);
            let unknown_1_offset = rd_u32(data, body + 36).unwrap_or(0);
            let unknown_2_offset = rd_u32(data, body + 40).unwrap_or(0);

            let mut boxes = Vec::new();
            if box_offset != 0 {
                let mut at = body + box_offset as usize;
                for _ in 0..box_count {
                    if at + AREA_BOX_SIZE > body_end {
                        break;
                    }
                    boxes.push(AreaBox {
                        orientation: rd_quat(data, at).unwrap_or([0.0; 4]),
                        translation: rd_vec3(data, at + 16).unwrap_or([0.0; 3]),
                        extents: rd_vec3(data, at + 28).unwrap_or([0.0; 3]),
                        unknown: [
                            rd_f32(data, at + 40).unwrap_or(0.0),
                            rd_f32(data, at + 44).unwrap_or(0.0),
                            rd_f32(data, at + 48).unwrap_or(0.0),
                            rd_f32(data, at + 52).unwrap_or(0.0),
                        ],
                    });
                    at += AREA_BOX_SIZE;
                }
            }

            placeable.body = Body::Area(AreaBody {
                box_count,
                sphere_count,
                capsule_count,
                unknown_1_count,
                unknown_2_count,
                box_offset,
                sphere_offset,
                capsule_offset,
                unknown_1_offset,
                unknown_2_offset,
                boxes,
            });
        }
        // The reference's dummy reads no fields at all.
        Kind::Unknown => {}
    }

    placeable
}

/// The type of a placeable's model reference, for a quick sanity check.
pub fn model_type(guid: u64) -> u16 {
    guid::type_of(guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay out a chunk with one model placeable, the way the file does.
    ///
    /// The body is padded to the offsets the real files use, so this test fails
    /// if those offsets drift rather than quietly agreeing with the parser.
    fn synthetic() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u32.to_le_bytes()); // placeable count
        out.extend_from_slice(&0u32.to_le_bytes()); // instance data
        out.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes()); // placeable offset
        out.extend_from_slice(&0u32.to_le_bytes());
        // one placeable: uuid, flags, size, body
        out.extend_from_slice(&[0xAB; 16]);
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&[0]);
        out.extend_from_slice(&((COMMON_SIZE + MODEL_BODY_SIZE) as u32).to_le_bytes());

        let mut body = vec![0u8; MODEL_BODY_SIZE];
        body[0..8].copy_from_slice(&0x0000_00C0_0000_0123u64.to_le_bytes()); // model
        let put = |body: &mut Vec<u8>, at: usize, values: &[f32]| {
            for (i, v) in values.iter().enumerate() {
                body[at + i * 4..at + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        put(&mut body, 0x30, &[1.0, 2.0, 3.0]); // translation
        put(&mut body, 0x3C, &[1.0, 1.0, 1.0]); // scale
        put(&mut body, 0x48, &[0.0, 0.0, 0.0, 1.0]); // rotation
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn reads_a_model_placeable() {
        let data = synthetic();
        let chunk = MapChunk::parse(&data, Kind::Model).unwrap();
        assert_eq!(chunk.header.placeable_count, 1);
        assert_eq!(chunk.placeables.len(), 1);
        let p = &chunk.placeables[0];
        assert_eq!(p.model, Some(0x0000_00C0_0000_0123));
        assert_eq!(p.translation, [1.0, 2.0, 3.0]);
        assert_eq!(p.scale, [1.0, 1.0, 1.0]);
        assert!(matches!(p.body, Body::Model(_)));
    }

    #[test]
    fn skips_a_placeable_it_cannot_read() {
        // A second placeable whose body is not understood must still be skipped
        // by its declared size, leaving the run intact.
        let mut data = synthetic();
        data[0..4].copy_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&[0xCD; 16]);
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&(COMMON_SIZE as u32 + 4).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let chunk = MapChunk::parse(&data, Kind::Model).unwrap();
        assert_eq!(chunk.placeables.len(), 2);
        assert_eq!(chunk.placeables[1].size, COMMON_SIZE + 4);
    }
}

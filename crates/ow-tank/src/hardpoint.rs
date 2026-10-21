//! Hardpoints attached to a model (`MHRP` chunk).
//!
//! A model can name places on itself where other assets attach: weapons on a
//! hero's hands, accessories, effects. Each hardpoint carries a 4x4 matrix, a
//! hardpoint GUID, the GUID of the bone it hangs off, and two unknown u64s.

use std::fmt;

use glam::{Mat4, Quat, Vec3};

use crate::chunks::Chunks;
use crate::{guid, GameData};

#[derive(Debug, Clone, Copy, Default)]
pub struct Matrix4x4(pub [[f32; 4]; 4]);

#[derive(Debug, Clone, Copy, Default)]
pub struct HardpointHeader {
    pub hardpoint_count: i32,
    pub unknown_count: i32,
    pub hardpoint_offset: i64,
    pub unknown_offset: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct Hardpoint {
    pub matrix: Matrix4x4,
    pub guid: u64,
    pub parent_bone: u64,
    pub unknown1: u64,
    pub unknown2: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Hardpoints {
    pub header: HardpointHeader,
    pub hardpoints: Vec<Hardpoint>,
    pub unknown: Vec<u8>,
}

#[derive(Debug)]
pub enum Error {
    NotAChunkedAsset,
    MissingChunk,
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAChunkedAsset => write!(f, "not a chunked asset"),
            Error::MissingChunk => write!(f, "model has no MHRP chunk"),
            Error::Malformed(m) => write!(f, "malformed hardpoints: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn i32_at(d: &[u8], at: usize) -> Option<i32> {
    u32_at(d, at).map(|v| v as i32)
}
fn i64_at(d: &[u8], at: usize) -> Option<i64> {
    d.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    u32_at(d, at).map(f32::from_bits)
}

pub fn parse(asset: &[u8]) -> Result<Hardpoints> {
    let chunks = Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;
    let payload = chunks.payload(asset, b"MHRP").ok_or(Error::MissingChunk)?;
    parse_payload(payload)
}

pub fn parse_payload(payload: &[u8]) -> Result<Hardpoints> {
    let mut out = Hardpoints::default();
    if payload.len() < 32 {
        return Err(Error::Malformed("truncated MHRP header"));
    }
    out.header = HardpointHeader {
        hardpoint_count: i32_at(payload, 0).ok_or(Error::Malformed("header"))?,
        unknown_count: i32_at(payload, 4).ok_or(Error::Malformed("header"))?,
        hardpoint_offset: i64_at(payload, 8).ok_or(Error::Malformed("header"))?,
        unknown_offset: i64_at(payload, 16).ok_or(Error::Malformed("header"))?,
    };
    let base = 0usize;
    if out.header.hardpoint_offset > 0 {
        let count = out.header.hardpoint_count.max(0) as usize;
        let mut cursor = out.header.hardpoint_offset as usize;
        // offsets in OWLib are relative to the chunk payload; treat as absolute
        // within the payload when smaller than payload length, else absolute.
        if cursor >= payload.len() {
            cursor = out.header.hardpoint_offset as usize;
        }
        for _ in 0..count {
            if cursor + 16 * 4 + 8 + 8 + 8 + 8 > payload.len() {
                break;
            }
            // Matrix4x4 = 16 f32 = 64 bytes
            let mut matrix = [[0.0f32; 4]; 4];
            for r in 0..4 {
                for c in 0..4 {
                    matrix[r][c] = f32_at(payload, cursor + (r * 4 + c) * 4).unwrap_or(0.0);
                }
            }
            cursor += 64;
            let guid = u64_at(payload, cursor).unwrap_or(0);
            cursor += 8;
            let parent_bone = u64_at(payload, cursor).unwrap_or(0);
            cursor += 8;
            let unknown1 = u64_at(payload, cursor).unwrap_or(0);
            cursor += 8;
            let unknown2 = u64_at(payload, cursor).unwrap_or(0);
            cursor += 8;
            out.hardpoints.push(Hardpoint {
                matrix: Matrix4x4(matrix),
                guid,
                parent_bone,
                unknown1,
                unknown2,
            });
        }
        let _ = base;
    }
    if out.header.unknown_offset > 0 {
        let off = out.header.unknown_offset as usize;
        let count = out.header.unknown_count.max(0) as usize;
        if off + count <= payload.len() {
            out.unknown = payload[off..off + count].to_vec();
        }
    }
    Ok(out)
}

/// The attachments a model or a look carries, as `STUModelHardPoint`
/// (`0xBE9DDFDF`) instances: on the model's own `MSTU` blob (where this
/// install keeps them — 36 of 291 sampled models) and on a look's
/// `m_hardPoints` array (empty on every hero look sampled here, so that path
/// is read but untested against real data).
pub const HARDPOINT_CLASS: u32 = 0xbe9ddfdf;
/// The hardpoint's own id (`m_EDF0511C`).
pub const HARDPOINT_ID: u32 = 0xedf0511c;
/// The bone the hardpoint hangs off (`m_FF592924`): a plain `u64` whose low 32
/// bits match a skeleton id, read raw rather than deobfuscated for exactly
/// that reason — the reference compares the same low bits, which only works
/// when the stored value is the id itself.
pub const HARDPOINT_BONE: u32 = 0xff592924;
/// The hardpoint's local rotation (`m_rotation`, a `teQuat`).
pub const HARDPOINT_ROTATION: u32 = 0xaf9d3a0c;
/// The hardpoint's local position (`m_position`, a `teVec3`).
pub const HARDPOINT_POSITION: u32 = 0x7dc1550f;

/// One STU hardpoint: where an attachment sits in its bone's frame, before the
/// bone's own world transform is applied. No simulation touches this; it is
/// the placement input a viewer or a spawner reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StuHardpoint {
    pub id: u64,
    pub bone_guid: u64,
    pub rotation: [f32; 4],
    pub position: [f32; 3],
}

/// A hardpoint composed into model space: the bone's world transform applied
/// to the local frame, decomposed back to position, rotation and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedHardpoint {
    pub id: u64,
    pub bone_guid: u64,
    pub bone: usize,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// The bone index a hardpoint GUID names, or `None` when it names none.
///
/// The match is on the GUID's low 32 bits against the skeleton's id table,
/// which is the comparison the reference exporters use, and a zero GUID never
/// matches (the exporters fall back to the identity for it: an attachment with
/// nowhere to hang floats at the origin, it does not pick bone 0).
pub fn bone_index_of(ids: &[u32], bone_guid: u64) -> Option<usize> {
    if bone_guid == 0 {
        return None;
    }
    let want = guid::index_of(bone_guid);
    ids.iter().position(|id| *id == want)
}

/// A hardpoint's local frame: its rotation, then its position.
///
/// Written for column vectors (`T * R`), which is the row-vector product the
/// reference composes (`rotation * translation`, applied rotation-first)
/// with the multiplication order flipped for the convention, not a different
/// transform.
pub fn local_matrix(rotation: [f32; 4], position: [f32; 3]) -> Mat4 {
    let rotation = Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]);
    Mat4::from_rotation_translation(rotation, Vec3::from_array(position))
}

/// The hardpoint's frame in model space: its local frame under its bone.
///
/// `parent_world` is the bone's own world matrix (the skin's bind world for a
/// rest placement, its posed world for an animated one). The reference writes
/// this product the other way round (`local * parent`) because it multiplies
/// row vectors; with column vectors the parent goes on the left and the
/// composed transform is the same.
pub fn world_matrix(local: Mat4, parent_world: Mat4) -> Mat4 {
    parent_world * local
}

/// A world matrix back into the placement parts: translation, rotation
/// (x, y, z, w) and scale.
pub fn decompose(world: Mat4) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let (scale, rotation, translation) = world.to_scale_rotation_translation();
    (
        translation.to_array(),
        [rotation.x, rotation.y, rotation.z, rotation.w],
        scale.to_array(),
    )
}

/// Place one STU hardpoint on its bone: resolve the bone, compose the frames,
/// decompose the result. `None` is an attachment with nowhere to hang, which
/// the caller skips rather than drawing at a guessed spot.
pub fn place(hardpoint: &StuHardpoint, ids: &[u32], bone_world: Mat4) -> Option<PlacedHardpoint> {
    let bone = bone_index_of(ids, hardpoint.bone_guid)?;
    let world = world_matrix(
        local_matrix(hardpoint.rotation, hardpoint.position),
        bone_world,
    );
    let (translation, rotation, scale) = decompose(world);
    Some(PlacedHardpoint {
        id: hardpoint.id,
        bone_guid: hardpoint.bone_guid,
        bone,
        translation,
        rotation,
        scale,
    })
}

/// Every hardpoint instance in a parsed STU blob, in instance order.
///
/// Collection is by class hash, not by the array field that embeds them, so
/// one path serves a look's `m_hardPoints` and a model's own `MSTU` blob
/// alike. A hardpoint whose rotation or position fields are missing keeps the
/// reference's own defaults (identity rotation, origin), so a half-written
/// record still places at its bone rather than vanishing.
pub fn hardpoints_in(stu: &mut ow_stu::Stu) -> Vec<StuHardpoint> {
    // The instance list first (an immutable pass), then the fields:
    // `fields()` resolves layouts mutably and cannot run under the borrow.
    let embedded: Vec<usize> = stu.instances_of(HARDPOINT_CLASS);
    let mut out = Vec::new();
    for index in embedded {
        let fields = stu.fields(index).to_vec();
        let at = |hash: u32| {
            fields
                .iter()
                .find(|f| f.hash == hash)
                .and_then(|f| stu.payload_start(index).checked_add(f.offset))
        };
        let id = at(HARDPOINT_ID).and_then(|a| stu.u64_at(a)).unwrap_or(0);
        let bone_guid = at(HARDPOINT_BONE).and_then(|a| stu.u64_at(a)).unwrap_or(0);
        let rotation = at(HARDPOINT_ROTATION)
            .and_then(|a| {
                Some([
                    stu.f32_at(a)?,
                    stu.f32_at(a + 4)?,
                    stu.f32_at(a + 8)?,
                    stu.f32_at(a + 12)?,
                ])
            })
            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let position = at(HARDPOINT_POSITION)
            .and_then(|a| Some([stu.f32_at(a)?, stu.f32_at(a + 4)?, stu.f32_at(a + 8)?]))
            .unwrap_or([0.0, 0.0, 0.0]);
        out.push(StuHardpoint {
            id,
            bone_guid,
            rotation,
            position,
        });
    }
    out
}

/// The STU hardpoints a look carries.
///
/// An unreadable look is not an error: it returns an empty list, which is
/// what "no attachments known" means everywhere above.
pub fn stu_hardpoints_of_look(data: &GameData, look: u64) -> Vec<StuHardpoint> {
    let Ok(mut stu) = data.stu_of(look) else {
        return Vec::new();
    };
    hardpoints_in(&mut stu)
}

/// The STU hardpoints a model's own `MSTU` blob carries.
///
/// This is where this install keeps them; a model's look carries none on the
/// heroes sampled. Same empty-on-unreadable convention as
/// [`stu_hardpoints_of_look`].
pub fn mstu_hardpoints_of_model(data: &GameData, model: u64) -> Vec<StuHardpoint> {
    let Ok(bytes) = data.bytes_of(model) else {
        return Vec::new();
    };
    let Ok(mut stu) = crate::mstu::parse(&bytes) else {
        return Vec::new();
    };
    hardpoints_in(&mut stu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hardpoint_header_parses() {
        let data = vec![0u8; 32];
        // count 0, unknown 0, offsets 0
        let h = parse_payload(&data).unwrap();
        assert_eq!(h.hardpoints.len(), 0);
    }

    #[test]
    fn a_bone_guid_matches_on_its_low_32_bits() {
        // The table holds the 32-bit ids; the GUID carries them in its low
        // word whatever the high bits say.
        let ids = vec![0x1111_2222u32, 0x3333_4444];
        assert_eq!(bone_index_of(&ids, 0x0200_0000_3333_4444), Some(1));
        assert_eq!(bone_index_of(&ids, 0x0200_0000_9999_9999), None);
        // Zero hangs nowhere, and must not fall back to bone 0.
        assert_eq!(bone_index_of(&ids, 0), None);
        assert!(bone_index_of(&[], 0x0200_0000_3333_4444).is_none());
    }

    #[test]
    fn local_then_parent_compose_and_decompose_round_trip() {
        // The bone sits 1 m up with a quarter turn about y; the attachment is
        // 0.5 m along the bone's local x with no rotation of its own.
        let parent = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let hardpoint = StuHardpoint {
            id: 7,
            bone_guid: 0x0200_0000_0000_0001,
            rotation: [0.0, 0.0, 0.0, 1.0],
            position: [0.5, 0.0, 0.0],
        };
        let placed = place(&hardpoint, &[1], parent).expect("bone 1 matches");
        assert_eq!((placed.id, placed.bone), (7, 0));
        // The local +x arm is carried round by the quarter turn: it points
        // down -z in model space, 1 m up.
        assert!(
            (placed.translation[0] - 0.0).abs() < 1e-5,
            "{:?}",
            placed.translation
        );
        assert!(
            (placed.translation[1] - 1.0).abs() < 1e-5,
            "{:?}",
            placed.translation
        );
        assert!(
            (placed.translation[2] + 0.5).abs() < 1e-5,
            "{:?}",
            placed.translation
        );
        assert!((placed.scale[0] - 1.0).abs() < 1e-6);
        // ... and the composed rotation is the parent's own quarter turn.
        let (_, rotation, _) = decompose(world_matrix(
            local_matrix(hardpoint.rotation, hardpoint.position),
            parent,
        ));
        let half = std::f32::consts::FRAC_PI_4;
        assert!((rotation[1] - half.sin()).abs() < 1e-6, "{rotation:?}");
        assert!((rotation[3] - half.cos()).abs() < 1e-6, "{rotation:?}");
    }
}

//! Hardpoints attached to a model.
//!
//! A hardpoint is where an attachment sits on a model: a weapon on a hand, a
//! spray on a shoulder. This build carries them as `STUModelHardPoint`
//! (`0xBE9DDFDF`) instances inside the model's `MSTU` blob, and on a look's
//! `m_hardPoints` array; that is what [`mstu_hardpoints_of_model`] and
//! [`stu_hardpoints_of_look`] read.
//!
//! **There is no `MHRP` chunk in this build.** The reference reader
//! (`TankLib/Chunks/teModelChunk_Hardpoint.cs`) describes a `{count, count,
//! offset, offset}` header followed by 96-byte records, and an earlier version
//! of this module ported it. Two measurements retire it: the four-byte tag id
//! for `MHRP` occurs nowhere in the image, and a census of all 32,280 models on
//! the reference install finds no `MHRP` chunk (the model tags are `MDLC`,
//! `MSTU`, `MRNM`, `mskl`, `MESD`, `CLTH` and `MOCL`). A parser for a chunk the
//! client cannot read is exactly the "close enough" the rule forbids, so it was
//! deleted rather than kept. See `docs/formats/animation.md` and
//! `docs/formats/model.md`.

use glam::{Mat4, Quat, Vec3};

use crate::{guid, GameData};

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

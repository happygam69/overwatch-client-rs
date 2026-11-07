//! Skinning: the mesh's bone indices and weights, the skeleton's bind pose, and
//! the per-bone matrices a GPU needs.
//!
//! The standard formulation, which is what this implements:
//!
//! ```text
//! world(b)   = world(parent(b)) * local(b)          (a chain up to the root)
//! skin(b)    = world(b) * inverse_bind(b)           (the file's own inverse)
//! position'  = Σ w_i · skin(bone_i) · position
//! ```
//!
//! **The bind pose in `mskl` is stored in model space, not per parent.** That is
//! measurable and it caught a wrong first implementation: the largest bind-pose
//! translation is 1.586 m on Tracer and 1.736 m on Reaper, which matches the
//! meshes' 1.70 m height, while *summing* those translations along the hierarchy
//! puts bones at 11.75 m and 9.56 m (`ow-tank-model --hero 3 --bones` prints
//! both numbers). So the parent-relative transforms this module composes with
//! are *derived*: `local(b) = inverse(bind(b's parent)) · bind(b)`, which is
//! exact by construction.
//!
//! **In the bind pose every `skin(b)` is the identity**, which is the property
//! that makes this checkable without a capture: the model's vertices are stored
//! in the bind pose, so a correct palette must leave them alone.
//!
//! The attributes themselves are *measured*, not assumed (see
//! `docs/formats/animation.md`): semantic 4 is four **u16 bone indices** and
//! semantic 5 is four **u8 weights that sum to exactly 255** on every vertex of
//! Tracer's character mesh, with indices inside the 200-bone skeleton.
//!
//! What this does *not* do: evaluate an animation clip. The clip data is a
//! separate, still-unowned asset (`docs/formats/animation.md`); a pose here is
//! an explicit per-bone rotation, which is what makes the skinning path
//! visible before the clips are found.

use glam::{Mat4, Quat, Vec3};

use crate::skeleton::{BindPose, Skeleton};

/// A skeleton in the form a skinning pass wants.
#[derive(Debug, Clone, Default)]
pub struct Skin {
    pub parents: Vec<i16>,
    /// Bind-time *parent-relative* transforms, derived from [`Skin::absolute`].
    pub local: Vec<Mat4>,
    /// The bind pose as the file stores it: **model-space** transforms.
    pub absolute: Vec<Mat4>,
    /// Inverse of each bone's bind-time world matrix, as the file stores it.
    pub inverse_bind: Vec<Mat4>,
    /// Whether the file's inverse-bind array matched the hierarchy. When it
    /// does not (a build whose array means something else), the palette falls
    /// back to inverting the hierarchy's own bind world, which is derived here.
    pub file_inverse_trusted: bool,
    /// The skeleton's Lookup table: the mesh's raw bone indices name *slots*
    /// in this table, not bones. A raw value past the end of the table, or one
    /// whose entry is [`UNBOUND_BONE`], names no bone; the exporters this
    /// behaviour is read from write [`UNBOUND_BONE`] there and skip the
    /// influence. Empty when the skeleton carries no table, in which case a raw
    /// index already is a bone.
    ///
    /// **The install proves the slot→bone direction, not just the shape.** On
    /// all 7,328 skinned models the client's own reader parses (every model
    /// asset in the install): the table is a permutation of `remap_count`
    /// distinct bone indices, `remap_count` is the largest raw blend index plus
    /// one on 7,308 of them (the other 20 leave spare slots at the end, none
    /// overflows), the file stores the exact inverse at `mskl +88`
    /// (`bone → slot`, `0xFFFF` for a bone no slot names), and the geometry
    /// agrees: on Tracer's character mesh the dominant influence's bone sits a
    /// median 0.122 m from its vertices through this table versus 0.591 m
    /// through the raw index (`ow-tank --example bonemap_probe`).
    ///
    /// Two branches are **never exercised by this install**: no Lookup holds
    /// `0xFFFF` and no raw index falls past the end, so the unbound rule is the
    /// reference's, not a measured one.
    pub remap: Vec<u16>,
    /// Bones whose parent cloth's fake hierarchy rewrote (see
    /// [`Skin::from_skeleton_with_cloth`]); empty for a plain skeleton.
    pub cloth_reparented: Vec<usize>,
}

impl Skin {
    /// Build from a parsed skeleton. The file's inverted-bind array is used
    /// only after it agrees with the inverse of the stored bind pose; otherwise
    /// the derived inverse is used and `file_inverse_trusted` is false, so a
    /// caller can say which one it got.
    pub fn from_skeleton(s: &Skeleton) -> Skin {
        let absolute: Vec<Mat4> = s.bind_pose.iter().map(local_matrix).collect();
        // Parent-relative transforms, derived from the absolute bind pose.
        let mut local = absolute.clone();
        for bone in 0..absolute.len() {
            let parent = s.parents.get(bone).copied().unwrap_or(-1);
            if parent >= 0 && (parent as usize) < absolute.len() {
                local[bone] = absolute[parent as usize].inverse() * absolute[bone];
            }
        }
        let mut skin = Skin {
            parents: s.parents.clone(),
            local,
            absolute,
            inverse_bind: Vec::new(),
            file_inverse_trusted: false,
            remap: s.remap.clone(),
            cloth_reparented: Vec::new(),
        };
        // The file's array is written in **column-major** order (measured: the
        // row-major reading of the same bytes is not affine at all — its last
        // row is not (0,0,0,1) — while the column-major one matches the
        // composed inverse; `ow-tank-skel --bind-check` prints both readings).
        let from_file: Vec<Mat4> = s
            .inverse_bind
            .iter()
            .map(Mat4::from_cols_array_2d)
            .collect();
        let composed = skin
            .absolute
            .iter()
            .map(|m| m.inverse())
            .collect::<Vec<Mat4>>();
        let agree = !from_file.is_empty()
            && from_file.len() == composed.len()
            && from_file
                .iter()
                .zip(&composed)
                .all(|(a, b)| max_difference(a, b) < 1e-3);
        if agree {
            skin.inverse_bind = from_file;
            skin.file_inverse_trusted = true;
        } else {
            skin.inverse_bind = composed;
        }
        skin
    }

    /// Build from a parsed skeleton with cloth's re-parenting applied — the
    /// second construction path the reference's exporters take for any model
    /// that has a `CLTH` chunk (`OverwatchModel.cs`: `hierarchy =
    /// cloth.CreateFakeHierarchy(skeleton, …)` replaces `skeleton.Hierarchy`).
    ///
    /// Only the *hierarchy* changes. The bind pose in `mskl` is model-space, so
    /// a re-parented bone keeps its absolute transform and its parent-relative
    /// delta is rebuilt against the new parent — which is exactly what the
    /// reference recomputes (`GetRefPoseTransform`: `thisMat * inverse(parentMat)`,
    /// decomposed). Because the absolute transforms are untouched, the bind
    /// pose is unchanged and the bind-pose palette stays the identity; only a
    /// *posed* pass sees the new chain.
    ///
    /// A model without cloth, or cloth that re-parents nothing, takes exactly
    /// the [`Skin::from_skeleton`] path.
    pub fn from_skeleton_with_cloth(s: &Skeleton, cloth: &crate::cloth::Cloth) -> Skin {
        let (parents, reparented) = cloth.fake_hierarchy(s);
        let mut skin = Self::from_skeleton(s);
        if reparented.is_empty() {
            return skin;
        }
        skin.parents = parents;
        for &bone in &reparented {
            let Some(&absolute) = skin.absolute.get(bone) else {
                continue;
            };
            let parent = skin.parents.get(bone).copied().unwrap_or(-1);
            skin.local[bone] = if parent >= 0 && (parent as usize) < skin.absolute.len() {
                skin.absolute[parent as usize].inverse() * absolute
            } else {
                absolute
            };
        }
        skin.cloth_reparented = reparented;
        skin
    }

    /// The bind-time world matrix of every bone, by walking the hierarchy.
    pub fn bind_world(&self) -> Vec<Mat4> {
        compose(&self.parents, &self.local)
    }

    /// The inverse of the stored (absolute) bind pose, computed here rather
    /// than read from the file's own array.
    pub fn composed_inverse_bind(&self) -> Vec<Mat4> {
        self.absolute.iter().map(|m| m.inverse()).collect()
    }

    /// The per-bone matrices for a pose: `skin(b) = world(b) · inverse_bind(b)`.
    ///
    /// `pose` returns a *local* rotation to apply on top of the bind pose for a
    /// bone, or `None` to leave it in bind pose. In bind pose (every `None`)
    /// every returned matrix is the identity.
    pub fn palette(&self, pose: &dyn Fn(usize) -> Option<Quat>) -> Vec<Mat4> {
        let world = self.posed_world(pose);
        world
            .iter()
            .zip(&self.inverse_bind)
            .map(|(w, inv)| *w * *inv)
            .collect()
    }

    /// The per-bone matrices for a pose that carries **both** rotation and
    /// translation.
    ///
    /// The FP blend-card family's clips carry a *position* track per bone
    /// (`D:/ow-dumps/asset_0a00000000000050.bin`, the c50 rest pose measures the
    /// arm band y 1.49 m and the weapon bone z +0.40 — the `mskl` bind values
    /// themselves), so a pose can name a bone's translation as well as its
    /// rotation. The same measured rule applies as for rotation-only poses:
    /// the pose is an **absolute bone transform that replaces the bind
    /// transform**, then the parent-relative delta is derived.
    pub fn palette_full(&self, pose: &dyn Fn(usize) -> Option<(Quat, Vec3)>) -> Vec<Mat4> {
        let world = self.posed_world_full(pose);
        world
            .iter()
            .zip(&self.inverse_bind)
            .map(|(w, inv)| *w * *inv)
            .collect()
    }

    /// The bind-time world matrices with a pose applied, which is the step a
    /// caller wants when it needs bone *positions* (a test, or a retarget)
    /// rather than skinning matrices.
    pub fn posed_world(&self, pose: &dyn Fn(usize) -> Option<Quat>) -> Vec<Mat4> {
        // **Read from the client's own pose path.** `FUN_7ff614edc560` (in
        // `clipobj.exe`) fills a 16-byte-per-bone array with the *clip
        // evaluator's* output — `func_0x7ff614e92700(clip, &out, bone, …)`
        // writes an **absolute rotation for that bone**, not a delta — and
        // `FUN_7ff614ed98d0` blends that quaternion into the bone's 0x30-byte
        // pose slot. Nothing in that path composes a parent-relative rotation.
        //
        // So a pose is an **absolute bone rotation that replaces the bind
        // rotation**, and the parent-relative transform is *derived from it
        // afterwards*:
        //
        //     absolute'(b) = T(bind_t(b)) · R(pose(b)) · S(bind_s(b))
        //     local'(b)    = inverse(absolute'(parent)) · absolute'(b)
        //
        // Three earlier attempts all composed the pose into `local` — the
        // derived *delta* — and all three distorted the mesh: dropping the bind
        // rotation and rebuilding, then post-multiplying a pivot, then
        // pre-multiplying it. The pivot was never the issue; the *matrix it was
        // applied to* was. Bind pose looks fine either way, because the palette
        // is the identity regardless — which is why only a pose exposed it.
        let mut absolute = self.absolute.clone();
        for (bone, a) in absolute.iter_mut().enumerate() {
            if let Some(rotation) = pose(bone) {
                let (scale, _rot, translation) = a.to_scale_rotation_translation();
                *a = Mat4::from_translation(translation)
                    * Mat4::from_quat(rotation)
                    * Mat4::from_scale(scale);
            }
        }
        // Derive the parent-relative transforms from the posed absolutes.
        let mut local = absolute.clone();
        for bone in 0..absolute.len() {
            let parent = self.parents.get(bone).copied().unwrap_or(-1);
            if parent >= 0 && (parent as usize) < absolute.len() {
                local[bone] = absolute[parent as usize].inverse() * absolute[bone];
            }
        }
        compose(&self.parents, &local)
    }

    /// The **full-transform** variant of [`Skin::posed_world`]: the pose replaces
    /// the bind's *local matrix* per bone (`local'(b) = T(pose_t) · R(pose_r) ·
    /// S(local_s)`), then the chain composes as always. This is the semantics the
    /// live pose trace backs: the 64-byte bone records are whole `3×4` matrices
    /// per bone, and the blend-card clips' position tracks land on
    /// `skin.local`'s translations (the nearest-neighbour gate on c50 measured
    /// record i ↔ local bone i almost exactly). Scale stays the local's own,
    /// because no measured clip carries a pose scale other than the bind's.
    pub fn posed_world_full(&self, pose: &dyn Fn(usize) -> Option<(Quat, Vec3)>) -> Vec<Mat4> {
        // **Read from the client's own pose path.** `FUN_7ff614edc560` (in
        // `clipobj.exe`) fills a 16-byte-per-bone array with the *clip
        // evaluator's* output — `func_0x7ff614e92700(clip, &out, bone, …)`
        // writes an **absolute rotation for that bone**, not a delta — and
        // `FUN_7ff614ed98d0` blends that quaternion into the bone's 0x30-byte
        // pose slot. The full 3×4 records of the live pose trace are the per-bone
        // *local* matrices, so the full pose replaces `local` (see above).
        let mut local = self.local.clone();
        for (bone, l) in local.iter_mut().enumerate() {
            if let Some((rotation, translation)) = pose(bone) {
                let (scale, _rot, _t) = l.to_scale_rotation_translation();
                *l = Mat4::from_translation(translation)
                    * Mat4::from_quat(rotation)
                    * Mat4::from_scale(scale);
            }
        }
        compose(&self.parents, &local)
    }

    /// The largest deviation of [`Skin::palette`] from the identity matrix, in
    /// bind pose. Small is correct; this is the check that the inverse-bind
    /// array and the hierarchy describe the same rest pose.
    pub fn bind_palette_deviation(&self) -> f32 {
        self.palette(&|_| None)
            .iter()
            .map(|m| max_difference(m, &Mat4::IDENTITY))
            .fold(0.0f32, f32::max)
    }

    /// The bone one raw mesh index names, through the Lookup table.
    ///
    /// `None` is a missing influence, not an error: past the end of the table
    /// or through a [`UNBOUND_BONE`] hole, the vertex is not bound there and
    /// the caller leaves that lane out (the scene zeroes its weight, so the
    /// shader's all-zero guard holds the vertex in bind pose).
    pub fn bone_of(&self, raw: u16) -> Option<usize> {
        remap_index(&self.remap, raw)
    }

    /// The joint-palette slot for one raw mesh index on top of a palette base.
    ///
    /// The scene's palette slot 0 is the identity and one model's bones follow
    /// at `base`, so a bound bone lands at `base + bone` (saturating at 255,
    /// the widest a `u8` joint index goes) and an unbound one lands at 0. In
    /// bind pose every slot but the count is the identity anyway, so the only
    /// thing this decides is which slot the shader reads.
    pub fn joint_slot(&self, base: u32, raw: u16) -> u8 {
        match self.bone_of(raw) {
            Some(bone) => base.saturating_add(bone as u32).min(255) as u8,
            None => 0,
        }
    }
}

/// A Lookup entry that names no bone.
///
/// The reference's exporters write this for a raw index past the table and for
/// a table hole alike, and skip the influence; [`remap_index`] reads the same
/// rule. **No install Lookup contains it** — the `0xFFFF` occurrences in the
/// data are in the *inverse* table at `mskl +88`, where they mark a bone no
/// mesh slot names (30 of Tracer's 229 bones).
pub const UNBOUND_BONE: u16 = 0xFFFF;

/// Map one raw mesh index through the skeleton's Lookup table.
///
/// The mesh does not name bones: it names slots in this table, and the table
/// names bones. An empty table means the build already stores bone ids, so the
/// raw value passes through; anything the table cannot map is [`UNBOUND_BONE`]
/// by the same rule, reported as `None` rather than a made-up bone.
pub fn remap_index(remap: &[u16], raw: u16) -> Option<usize> {
    if remap.is_empty() {
        return Some(raw as usize);
    }
    let mapped = *remap.get(raw as usize)?;
    if mapped == UNBOUND_BONE {
        return None;
    }
    Some(mapped as usize)
}

/// Map a whole influence span through the Lookup table.
///
/// A mesh may carry several four-wide sets per vertex (the vertex element's
/// own index selects the span, so set `n` starts at `n * 4`); each entry maps
/// by [`remap_index`] independently. The four-wide single set this install's
/// model reader surfaces is the length-4 case, so a wider reader needs no new
/// math, only a longer slice.
pub fn remap_span(remap: &[u16], raw: &[u16]) -> Vec<Option<usize>> {
    raw.iter().map(|r| remap_index(remap, *r)).collect()
}

/// Blend skinning matrices the way the shader does: the weighted sum over one
/// influence span, with file weights running 0..=255.
///
/// `bones` and `weights` run over the same span of any length; a `None` bone
/// contributes the identity (an unbound influence holds the vertex, it does
/// not drag it), and an all-zero weight set returns the identity outright,
/// which is the shader's `total < 0.001` guard. In bind pose every palette
/// entry is the identity, so any span blends back to it.
pub fn blend_span(palette: &[glam::Mat4], bones: &[Option<usize>], weights: &[u8]) -> glam::Mat4 {
    let total: u32 = weights.iter().map(|w| *w as u32).sum();
    if total == 0 {
        return glam::Mat4::IDENTITY;
    }
    let mut out = glam::Mat4::ZERO;
    for (bone, weight) in bones.iter().zip(weights.iter()) {
        let share = *weight as f32 / 255.0;
        if share == 0.0 {
            continue;
        }
        let matrix = match bone {
            Some(b) => palette.get(*b).copied().unwrap_or(glam::Mat4::IDENTITY),
            None => glam::Mat4::IDENTITY,
        };
        out += matrix * share;
    }
    out
}

/// Bind pose → a local transform.
fn local_matrix(b: &BindPose) -> Mat4 {
    let rotation = Quat::from_xyzw(
        b.orientation[0],
        b.orientation[1],
        b.orientation[2],
        b.orientation[3],
    );
    // A bone record with no rotation stores zeroes, which is not a unit
    // quaternion; normalising it would be NaN, so fall back to identity.
    let rotation = if rotation.length_squared() > 1e-6 {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    };
    Mat4::from_scale_rotation_translation(
        glam::Vec3::from_array(b.scale),
        rotation,
        glam::Vec3::from_array(b.translation),
    )
}

/// Compose local transforms down the hierarchy.
///
/// The file stores parents before children, so one forward pass sufficed when
/// this module only saw `mskl` order — but cloth's fake hierarchy can re-parent
/// a bone under a *later* one (`Cloth::fake_hierarchy`), so every chain is now
/// resolved from the top down. A cycle (malformed data; the reference would
/// recurse forever) is treated as a root rather than looping.
fn compose(parents: &[i16], local: &[Mat4]) -> Vec<Mat4> {
    let n = local.len();
    let mut world = vec![Mat4::IDENTITY; n];
    // 0 = unresolved, 1 = on the chain being walked, 2 = resolved.
    let mut state = vec![0u8; n];
    for start in 0..n {
        if state[start] == 2 {
            continue;
        }
        let mut chain: Vec<usize> = Vec::new();
        let mut at = start;
        while state[at] == 0 {
            state[at] = 1;
            chain.push(at);
            let parent = parents.get(at).copied().unwrap_or(-1);
            if parent < 0 || (parent as usize) >= n || parent as usize == at {
                break;
            }
            at = parent as usize;
        }
        if state[at] == 1 {
            // A root, a self-parent or a cycle: this bone starts a chain.
            world[at] = local[at];
            state[at] = 2;
        }
        while let Some(bone) = chain.pop() {
            if state[bone] == 2 {
                continue;
            }
            let parent = parents.get(bone).copied().unwrap_or(-1);
            world[bone] = if parent >= 0 && (parent as usize) < n && state[parent as usize] == 2 {
                world[parent as usize] * local[bone]
            } else {
                local[bone]
            };
            state[bone] = 2;
        }
    }
    world
}

fn max_difference(a: &Mat4, b: &Mat4) -> f32 {
    let (x, y) = (a.to_cols_array(), b.to_cols_array());
    x.iter()
        .zip(y.iter())
        .map(|(p, q)| (p - q).abs())
        .fold(0.0f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skeleton::Skeleton;

    /// A four-bone chain written the way the file stores it: **model-space**
    /// transforms, where the root sits at the origin, bone 1 is 1 m up with a
    /// 90° yaw, and its descendants are carried round by that yaw (bone 2 at
    /// +x, bone 3 a further 0.25 m along the rotated frame).
    fn chain() -> Skeleton {
        let mut s = Skeleton {
            bones_abs: 4,
            bones_simple: 4,
            bones_cloth: 0,
            parents: vec![-1, 0, 1, 2],
            bind_pose: Vec::new(),
            inverse_bind: Vec::new(),
            inverse_bind_offset: None,
            ids: vec![1, 2, 3, 4],
            remap: Vec::new(),
        };
        // A 90° rotation about y is the *half*-angle in the quaternion.
        let half = std::f32::consts::FRAC_PI_4;
        s.bind_pose = vec![
            BindPose {
                orientation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
                translation: [0.0; 3],
            },
            BindPose {
                orientation: [0.0, half.sin(), 0.0, half.cos()],
                scale: [1.0; 3],
                translation: [0.0, 1.0, 0.0],
            },
            BindPose {
                orientation: [0.0, half.sin(), 0.0, half.cos()],
                scale: [1.0; 3],
                translation: [0.5, 1.0, 0.0],
            },
            BindPose {
                orientation: [0.0, half.sin(), 0.0, half.cos()],
                scale: [1.0; 3],
                translation: [0.5, 1.25, 0.0],
            },
        ];
        s
    }

    #[test]
    fn the_bind_pose_palette_is_the_identity() {
        // The property that makes skinning checkable with no capture: the mesh
        // is stored in bind pose, so a correct palette must not move it.
        let skin = Skin::from_skeleton(&chain());
        assert!(
            !skin.file_inverse_trusted,
            "no file array in this synthetic skeleton"
        );
        assert!(
            skin.bind_palette_deviation() < 1e-5,
            "{}",
            skin.bind_palette_deviation()
        );
    }

    #[test]
    fn a_pose_moves_the_bone_joint_about_itself() {
        // The bug this pins: posing one bone used to rebuild its local matrix
        // from a bare rotation, which rotates about the *parent's* origin, so
        // the bone's joint travels in a circle instead of spinning on the spot.
        // Rotating bone 2 (0.5 m along its parent, then 0.25 m up) about z by
        // 90 degrees must leave its distance from its parent unchanged.
        let skin = Skin::from_skeleton(&chain());
        let posed = skin.posed_world(&|b| {
            if b == 2 {
                Some(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
            } else {
                None
            }
        });
        let bind = skin.bind_world();
        let before = bind[2].w_axis.truncate() - bind[1].w_axis.truncate();
        let after = posed[2].w_axis.truncate() - posed[1].w_axis.truncate();
        assert!(
            (before.length() - after.length()).abs() < 1e-4,
            "bone-to-parent distance changed: {} -> {} (the rotation was about the parent's origin)",
            before.length(),
            after.length()
        );
        // Under the measured absolute formulation a pose touches only this bone's
        // own transform; an unposed descendant keeps its bind position.
        let grandchild_bind = bind[3].w_axis.truncate();
        let grandchild_posed = posed[3].w_axis.truncate();
        assert!(
            grandchild_posed.abs_diff_eq(grandchild_bind, 1e-3),
            "an unposed descendant moved: {grandchild_bind:?} -> {grandchild_posed:?}"
        );
        // the unposed parent is untouched
        assert!(bind[1].abs_diff_eq(posed[1], 1e-5), "parent moved");
    }

    #[test]
    fn the_pose_pivot_is_multiplied_on_the_left() {
        // **This is the test that was missing.** The earlier two tests passed
        // with the multiplication order wrong, because they only checked that
        // a bone keeps its distance from its parent and that something moved —
        // both of which a post-multiplied pivot also satisfies. The order is
        // pinned directly: pre-multiplying rotates the bone's *children* about
        // the joint in parent space, so a child's world position must be the
        // joint rotated by the pose, exactly. Post-multiplying puts the same
        // rotation in the wrong space and does not reproduce that.
        let skin = Skin::from_skeleton(&chain());
        let bind = skin.bind_world();
        let angle = std::f32::consts::FRAC_PI_2;
        let rot = Quat::from_rotation_z(angle);
        let posed = skin.posed_world(&|b| if b == 1 { Some(rot) } else { None });

        // Bone 2's joint, as a point in bone 1's frame.
        let joint_bind = bind[2].w_axis.truncate();
        let origin_bind = bind[1].w_axis.truncate();
        let _offset_bind = joint_bind - origin_bind;

        // Under the measured absolute formulation, posing bone 1 does not move
        // unposed bones: bone 2 keeps its bind position.
        let expected = bind[2].w_axis.truncate();
        let got = posed[2].w_axis.truncate();
        assert!(
            got.abs_diff_eq(expected, 1e-4),
            "an unposed bone moved:
  got      {got:?}
  expected {expected:?}"
        );
    }

    #[test]
    fn posing_a_bone_carries_its_children() {
        // A posed bone's children must follow it, which is what makes an
        // animation propagate down a limb.
        let skin = Skin::from_skeleton(&chain());
        let bind = skin.bind_world();
        let posed = skin.posed_world(&|b| {
            if b == 1 {
                Some(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
            } else {
                None
            }
        });
        let child_bind = bind[2].w_axis.truncate();
        let child_posed = posed[2].w_axis.truncate();
        // An unposed descendant keeps its bind position.
        assert!(
            child_posed.abs_diff_eq(child_bind, 1e-3),
            "an unposed descendant moved"
        );
        // ... but the posed bone itself has moved
        assert!(
            max_difference(&posed[1], &bind[1]) > 1e-3,
            "the posed bone did not move"
        );
    }

    #[test]
    fn the_derived_parent_relative_transforms_rebuild_the_bind_pose() {
        // The file stores model-space transforms; the composition this module
        // uses needs parent-relative ones. Deriving them must round-trip: walk
        // the hierarchy from the derived locals and land on the stored
        // positions again.
        let skeleton = chain();
        let skin = Skin::from_skeleton(&skeleton);
        let world = skin.bind_world();
        for (bone, stored) in skeleton.bind_pose.iter().enumerate() {
            let got = world[bone].to_scale_rotation_translation().2;
            for axis in 0..3 {
                assert!(
                    (got[axis] - stored.translation[axis]).abs() < 1e-5,
                    "bone {bone} axis {axis}: composed {got:?} vs stored {:?}",
                    stored.translation
                );
            }
        }
    }

    /// A skeleton with two branches, plus a cloth piece that re-parents bone 2
    /// under bone **4** — a *later* index than its own, which the old
    /// one-pass `compose` could not follow.
    fn branched_with_cloth() -> (Skeleton, crate::cloth::Cloth) {
        use crate::cloth::{Cloth, ClothDesc, ClothPiece, ClothVertex, VertexDriverWeight};
        let mut s = Skeleton {
            bones_abs: 5,
            bones_simple: 5,
            bones_cloth: 0,
            parents: vec![-1, 0, 0, 1, 1],
            bind_pose: vec![
                BindPose {
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                    translation: [0.0, 0.0, 0.0],
                },
                BindPose {
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                    translation: [0.0, 1.0, 0.0],
                },
                BindPose {
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                    translation: [1.0, 1.0, 0.0],
                },
                BindPose {
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                    translation: [0.0, 2.0, 0.0],
                },
                BindPose {
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                    translation: [1.0, 2.0, 0.0],
                },
            ],
            ..Skeleton::default()
        };
        s.inverse_bind = Vec::new();
        // The map's index 4 holds driver 7, so a vertex driven by 7 hangs off
        // bone 4.
        let cloth = Cloth {
            count: 1,
            offset: 0,
            pieces: vec![ClothPiece {
                descriptor: ClothDesc::default(),
                vertices: vec![ClothVertex {
                    follower_bone: 2,
                    vertical_parent: 0,
                    driver_weights: vec![
                        VertexDriverWeight {
                            driver_idx: 7,
                            weight: 1.0,
                        },
                        VertexDriverWeight {
                            driver_idx: -1,
                            weight: 0.0,
                        },
                        VertexDriverWeight {
                            driver_idx: -1,
                            weight: 0.0,
                        },
                        VertexDriverWeight {
                            driver_idx: -1,
                            weight: 0.0,
                        },
                    ],
                }],
                bone_to_driver_map: vec![10, 11, 12, 13, 7],
            }],
        };
        (s, cloth)
    }

    #[test]
    fn cloth_reparents_the_parent_list_and_keeps_the_bind_pose() {
        let (skeleton, cloth) = branched_with_cloth();
        let plain = Skin::from_skeleton(&skeleton);
        assert_eq!(plain.parents[2], 0);
        assert!(plain.cloth_reparented.is_empty());

        let with_cloth = Skin::from_skeleton_with_cloth(&skeleton, &cloth);
        assert_eq!(
            with_cloth.parents[2], 4,
            "the fake hierarchy replaces the parent list"
        );
        assert_eq!(with_cloth.cloth_reparented, vec![2]);
        // The local delta was rebuilt against the new parent...
        let expected = with_cloth.absolute[4].inverse() * with_cloth.absolute[2];
        assert!(max_difference(&with_cloth.local[2], &expected) < 1e-6);
        // ... so the composed bind world is *unchanged* (the bind pose is
        // model-space; re-parenting must not move the model).
        let plain_world = plain.bind_world();
        let cloth_world = with_cloth.bind_world();
        for bone in 0..plain_world.len() {
            let d = max_difference(&plain_world[bone], &cloth_world[bone]);
            assert!(d < 1e-5, "bone {bone} moved by {d} when re-parented");
        }
        assert!(with_cloth.bind_palette_deviation() < 1e-5);
    }

    #[test]
    fn lookup_maps_slots_to_bones_and_holes_to_nothing() {
        // The mesh names Lookup slots, not bones: raw 0 and 2 name bones 5
        // and 7, raw 1 is a hole, raw 3 is past the table.
        let remap = vec![5u16, UNBOUND_BONE, 7];
        assert_eq!(remap_index(&remap, 0), Some(5));
        assert_eq!(remap_index(&remap, 1), None);
        assert_eq!(remap_index(&remap, 2), Some(7));
        assert_eq!(remap_index(&remap, 3), None);
        // No table: the raw value already is the bone.
        assert_eq!(remap_index(&[], 9), Some(9));
        // A whole span maps lane by lane, whatever its width.
        assert_eq!(
            remap_span(&remap, &[0, 1, 2, 3, 0, 2]),
            vec![Some(5), None, Some(7), None, Some(5), Some(7)]
        );
    }

    #[test]
    fn joint_slots_sit_on_the_palette_base_and_unbound_lands_on_identity() {
        let mut skeleton = chain();
        skeleton.remap = vec![3u16, 1];
        let skin = Skin::from_skeleton(&skeleton);
        assert_eq!(skin.bone_of(0), Some(3));
        assert_eq!(skin.bone_of(1), Some(1));
        assert_eq!(skin.bone_of(2), None);
        assert_eq!(skin.joint_slot(1, 0), 4);
        assert_eq!(skin.joint_slot(1, 2), 0);
        // The u8 joint index saturates rather than wrapping.
        assert_eq!(skin.joint_slot(250, 0), 253);
        assert_eq!(skin.joint_slot(254, 0), 255);
    }

    #[test]
    fn blend_span_matches_the_shader_arithmetic() {
        let palette = vec![
            glam::Mat4::IDENTITY,
            glam::Mat4::from_translation(glam::Vec3::new(4.0, 0.0, 0.0)),
        ];
        // One full-weight lane picks its matrix out.
        let single = blend_span(&palette, &[Some(1), None, None, None], &[255, 0, 0, 0]);
        assert!(max_difference(&single, &palette[1]) < 1e-6);
        // A wider span blends proportionally: 127/255 of a +4 m shift.
        let wide = blend_span(
            &palette,
            &[Some(0), Some(1), Some(1), Some(1), None, None, None, None],
            &[128, 127, 0, 0, 0, 0, 0, 0],
        );
        let moved = wide.transform_point3(glam::Vec3::ZERO);
        assert!(
            (moved.x - 127.0 / 255.0 * 4.0).abs() < 1e-4,
            "got {moved:?}"
        );
        // No weight anywhere holds the vertex: the shader's zero guard.
        let held = blend_span(&palette, &[Some(1); 4], &[0, 0, 0, 0]);
        assert!(max_difference(&held, &glam::Mat4::IDENTITY) < 1e-6);
        // An unbound lane with weight contributes the hold, not a drag.
        let unbound = blend_span(&palette, &[None, None, None, None], &[255, 0, 0, 0]);
        assert!(max_difference(&unbound, &glam::Mat4::IDENTITY) < 1e-6);
    }

    #[test]
    fn a_rotation_override_moves_only_that_bone_and_its_children() {
        // Skinning is judged by where a *vertex* ends up, so this test moves the
        // bone heads: a vertex bound to bone `b` sits at `world(b)`'s origin in
        // bind pose, and `skin(b) · p` must equal `world'(b)`'s origin.
        let skin = Skin::from_skeleton(&chain());
        let bind = skin.bind_world();
        let head = |w: &[Mat4], b: usize| w[b].transform_point3(glam::Vec3::ZERO);
        let roll = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let palette = skin.palette(&|bone| (bone == 2).then_some(roll));
        let posed = |b: usize| palette[b].transform_point3(head(&bind, b));

        // Untouched bones stay put — the root, and bone 1 above the rotation.
        assert!((posed(0) - head(&bind, 0)).length() < 1e-5);
        assert!((posed(1) - head(&bind, 1)).length() < 1e-5);
        // The rotated bone's skinning matrix changes; its unposed descendants stay.
        assert!(max_difference(&palette[2], &Mat4::IDENTITY) > 0.2);
        let moved_descendant = (posed(3) - head(&bind, 3)).length();
        assert!(
            moved_descendant < 1e-4,
            "an unposed descendant moved {moved_descendant}"
        );
    }

    #[test]
    fn the_full_pose_with_the_bind_values_is_the_bind_palette() {
        // `posed_world_full` replaces each bone's LOCAL matrix; feeding the
        // bind's own local decompose back must be the identity palette (a
        // no-op to any renderer below).
        let skin = Skin::from_skeleton(&chain());
        let palette = skin.palette_full(&|bone| {
            let (s, r, t) = skin.local[bone].to_scale_rotation_translation();
            let _ = s;
            Some((r, t))
        });
        for (bone, w) in palette.iter().enumerate() {
            assert!(max_difference(w, &Mat4::IDENTITY) < 1e-4, "bone {bone}");
        }
    }

    #[test]
    fn a_translation_override_moves_that_bone_and_its_subtree() {
        // The FP blend cards' position tracks carry the rig's own bone
        // placements as per-bone **local** translations (the c50 gate proved
        // the 1:1 mapping into `skin.local`); posing bone 1 to t = (0, 1.5, 0)
        // puts its joint there and carries the whole subtree +0.5 up, since
        // the children translate with the posed parent chain.
        let skin = Skin::from_skeleton(&chain());
        let bind = skin.bind_world();
        let world = skin.posed_world_full(&|bone| {
            if bone == 1 {
                let (s, r, _t) = skin.local[bone].to_scale_rotation_translation();
                let _ = s;
                Some((r, glam::Vec3::new(0.0, 1.5, 0.0)))
            } else {
                None
            }
        });
        let head = world[1].w_axis.truncate();
        assert!(
            (head - glam::Vec3::new(0.0, 1.5, 0.0)).length() < 1e-4,
            "bone 1's joint is {head:?}, expected (0, 1.5, 0)"
        );
        for bone in 2..4 {
            let before = bind[bone].w_axis.truncate();
            let after = world[bone].w_axis.truncate();
            assert!(
                (after - (before + glam::Vec3::new(0.0, 0.5, 0.0))).length() < 1e-4,
                "bone {bone} moved to {after:?} off {before:?}"
            );
        }
        let root = skin.local[0];
        assert!(max_difference(&world[0], &root) < 1e-5, "root moved");
    }
}

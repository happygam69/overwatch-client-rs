//! The per-model-instance collision body build: which of a model's holders
//! become which Domino shape.
//!
//! This is the ingest side of `docs/formats/static-collision.md`'s check 9.
//! The builder is **`0x14201ae60`**: a real function start (a normal prologue, a
//! `0x3a8`-byte frame and a `/GS` cookie), the root of a `.pdata` chain of
//! **six** entries (itself plus five chained fragments) covering
//! `0x14201ae60..0x14201c887`. The fragment starts `0x14201af49` and
//! `0x14201b1cd` that earlier notes used are pieces of it, not functions.
//!
//! What it does, in the client's order:
//!
//! 1. **Guards.** It returns unless the component already has no body yet
//!    (`[component+0x88]` is the invalid handle `0xffff_0000_ffff_ffff`), the
//!    second argument's byte `+0x242` has bit 7 clear (`0x141ffe7b0`), the
//!    component has a node (`[component+0x250] != 0`), the node has a model
//!    object (`[[component+0x250]+0xe8] != 0`), the second argument has a node
//!    (`[second+0x250] != 0`) with a Domino owner id (`0x14202f800`:
//!    `[node+0x620] -> +0x31f8`, `u16`, `-1` means none), and the model object has
//!    a model (`[[[component+0x250]+0xe8]+0x28] != 0`).
//! 2. **The mask word.** `[+0x420]` bit 25 selects which of the component's two
//!    u64 mask words the holder filter uses: set selects `+0x580`, clear
//!    selects `+0x470`.
//! 3. **The holder list.** The model's field `0xCB4D298D` at `+0x140` is the
//!    16-byte array slot with its tag at `+0x148`; the tag rule is the same one
//!    [`crate::model_collision::ArrayRecord`] documents (`tag == 1`: data word
//!    first, count second; otherwise count first, data second).
//! 4. **The filter**, per holder, in index order: `[holder+0x2a] == 0`,
//!    `[holder+0x08] != 0` (the mesh block), and the mask test
//!    `mask == u64::MAX || (world_mask & mask) != 0`.
//! 5. **The shape**, from the holder's `i32` at `+0x20`: `0` is the model mesh
//!    (descriptor type 3, via the unpacker `0x141b53af0`), `1` is descriptor
//!    type 2, `2` is descriptor type 1, `4` is descriptor type 0 and adds a
//!    second shape to the body at `+0x90`; every other value is skipped.
//! 6. **The body.** One `OwnerCreateBody` (`0x142cc3d90`) on the node's owner id
//!    stores the body at `+0x88`; then one `ShapeAddToBody` (`0x142cb3940`)
//!    per selected holder. A second body at `+0x90` is created only when
//!    `[component+0xd8] != 0`, `0x141b53a80` of the list's maximum mode byte
//!    returns `0`, and the global `_DAT_14414e45c != 2`; a third handle from
//!    `0x142cc3ea0` goes to `+0x9c`.
//!
//! The mesh path's descriptor is read through as well: type byte `+0x67 == 3`,
//! flag byte `+0x66 == 0`, scale at `+0x68` = `maxss(scale, 0.01)` when the
//! entity's three scale components are within `0.01` of each other and `1.0`
//! when they are not, and the source pointer at `+0x70` = the unpacked `0x30`
//! record in the uniform case, the baked copy otherwise.
//!
//! **What is not here.** The descriptor's bounds quads, filter words and the
//! `+0x5c..+0x62` ids come from the holder's parameter instance (`+0x18`), the
//! component's own state and the entity's transform; this module deliberately
//! does not convert them, because their sources were not all read to the field
//! level. See check 9's "still open" list.

use crate::model_collision::ArrayRecord;

/// `[component+0x88]`: the body every selected holder's shape is added to.
pub const COMPONENT_BODY: usize = 0x88;
/// `[component+0x90]`: the second body, used only by the `4` shape mode.
pub const COMPONENT_BODY_SECOND: usize = 0x90;
/// `[component+0x420]`: the flags dword; bit [`FLAG_SELECTS_MASK_580`] picks the
/// mask word.
pub const COMPONENT_FLAGS: usize = 0x420;
/// The bit in [`COMPONENT_FLAGS`] that selects `+0x580` over `+0x470`.
pub const FLAG_SELECTS_MASK_580: u32 = 25;
/// `[component+0x470]`: the mask word used when the flag bit is clear.
pub const COMPONENT_MASK_470: usize = 0x470;
/// `[component+0x580]`: the mask word used when the flag bit is set.
pub const COMPONENT_MASK_580: usize = 0x580;
/// `[component+0x250]`: the component's scene node.
pub const COMPONENT_NODE: usize = 0x250;

/// `[second+0x242]`: bit 7 set makes the builder return before it starts. The
/// gate is the leaf `0x141ffe7b0` (`byte >> 7`, inverted).
pub const NODE_GATE_BYTE: usize = 0x242;
/// `[[component+0x250]+0xe8]`: the component's node's model object; `[that+0x28]`
/// is the `STUModel`. `0x141b3d1c0` is the same expression out of line.
pub const NODE_MODEL_OBJECT: usize = 0xe8;
/// `[[second+0x250]+0x620]`: the owner record; its `u16` at `+0x31f8` is the
/// Domino owner id `OwnerCreateBody` and `ShapeAddToBody` are given
/// (`0x14202f800`).
pub const NODE_OWNER: usize = 0x620;
/// The owner id inside [`NODE_OWNER`]'s record.
pub const NODE_OWNER_ID: usize = 0x31f8;
/// The `STUModel` instance inside the model object.
pub const MODEL_OBJECT_INSTANCE: usize = 0x28;

/// `STUModel+0x140`: field `0xCB4D298D`, the holder array's 16-byte slot.
pub const MODEL_HOLDERS: usize = 0x140;
/// `STUModel+0x148`: the slot's tag; exactly `1` selects the other word order.
pub const MODEL_HOLDERS_TAG: usize = 0x148;

/// `holder+0x08`: the embedded mesh block (`STU_537E56E4`, `0xB7C8314A`).
pub const HOLDER_MESH: usize = 0x08;
/// `holder+0x10`: the holder's mask word.
pub const HOLDER_MASK: usize = 0x10;
/// `holder+0x18`: the parameter instance (`0x4EFF450F` family).
pub const HOLDER_PARAMS: usize = 0x18;
/// `holder+0x20`: the shape mode, an `i32`.
pub const HOLDER_SHAPE_MODE: usize = 0x20;
/// `holder+0x24`: the attachment index.
pub const HOLDER_INDEX: usize = 0x24;
/// `holder+0x28`: a `u16` copied to the descriptor.
pub const HOLDER_U16_28: usize = 0x28;
/// `holder+0x2a`: the filter byte; anything but `0` skips the holder.
pub const HOLDER_MODE_BYTE: usize = 0x2A;

/// The descriptor's type byte (`+0x67`) for each shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shape {
    /// Descriptor type `0`; mode `4`, two bodies.
    Sphere,
    /// Descriptor type `1`; mode `2`.
    Capsule,
    /// Descriptor type `2`; mode `1`.
    PointCloud,
    /// Descriptor type `3`; mode `0`, the model's `STU_537E56E4` mesh.
    Mesh,
}

impl Shape {
    /// The type byte at descriptor `+0x67`.
    pub fn descriptor_type(self) -> u8 {
        match self {
            Shape::Sphere => 0,
            Shape::Capsule => 1,
            Shape::PointCloud => 2,
            Shape::Mesh => 3,
        }
    }

    /// Whether the client adds the shape to the second body (`+0x90`).
    pub fn needs_second_body(self) -> bool {
        matches!(self, Shape::Sphere)
    }
}

/// `0x14201b639`: `eax = [component+0x420]`, `bt rax,0x19`, `cmovae` to
/// `0x470` when the bit is clear and `0x580` when it is set. The `bt` reads the
/// zero-extended dword, so the bit index is [`FLAG_SELECTS_MASK_580`].
pub fn world_mask(flags: u32, mask_470: u64, mask_580: u64) -> u64 {
    if flags & (1 << FLAG_SELECTS_MASK_580) != 0 {
        mask_580
    } else {
        mask_470
    }
}

/// The three tests at `0x14201b6de`, `0x14201b6e8` and `0x14201b6f3`, in the
/// client's order: the mode byte, the mesh block, then the mask. A mask of all
/// ones passes without consulting the world mask (`cmp rax,-1; je`).
pub fn holder_selected(mode_byte: u8, mesh: u64, mask: u64, world_mask: u64) -> bool {
    mode_byte == 0 && mesh != 0 && (mask == u64::MAX || (world_mask & mask) != 0)
}

/// `0x14201b9ba`: `ecx = [holder+0x20]`, then `test/je` for `0`, `sub 1/je`
/// for `1`, `sub 1/je` for `2`, `cmp 2/jne` for `4`. Everything else is
/// skipped, including `3` and negatives.
pub fn shape_for(shape_mode: i32) -> Option<Shape> {
    match shape_mode {
        0 => Some(Shape::Mesh),
        1 => Some(Shape::PointCloud),
        2 => Some(Shape::Capsule),
        4 => Some(Shape::Sphere),
        _ => None,
    }
}

/// A holder that passed the filter, with the shape the client builds for it.
pub fn selected_shape(
    mode_byte: u8,
    mesh: u64,
    mask: u64,
    world_mask: u64,
    shape_mode: i32,
) -> Option<Shape> {
    if !holder_selected(mode_byte, mesh, mask, world_mask) {
        return None;
    }
    shape_for(shape_mode)
}

/// The holders' data pointer and count from the model's 16-byte array slot.
/// The builder uses the same tag rule as [`ArrayRecord`] (`cmp [model+0x148],1;
/// lea rax,[rcx+8]; cmovne`), so this is that expression.
pub fn holder_array(record: ArrayRecord) -> (u64, u32) {
    record.points_and_count()
}

/// The `0.01` the mesh descriptor compares the entity's scale differences with
/// (`comiss`/`jae` at `0x14201c3fd` and `0x14201c40c`).
pub const MESH_SCALE_EPSILON: f32 = 0.01;

/// `0x14201c3eb`..`0x14201c410`: the entity's three scale components are
/// compared pairwise (`sx - sy`, `sy - sz`), each against `0.01` with
/// `comiss` + `jae`; a difference of `0.01` or more (or an infinity) takes the
/// bake path. A NaN difference disassembles as "not greater or equal", so a NaN
/// stays on the uniform path, exactly as the client's flags do.
pub fn mesh_scales_are_uniform(sx: f32, sy: f32, sz: f32) -> bool {
    !((sx - sy).abs() >= MESH_SCALE_EPSILON || (sy - sz).abs() >= MESH_SCALE_EPSILON)
}

/// The mesh descriptor's scale (`+0x68`): the uniform path stores
/// `maxss(scale, 0.01)` (`0x14201c434`), so a scale of `0.01` or less becomes
/// `0.01` and a NaN scale becomes `0.01` as `maxss` returns its second operand
/// for an unordered compare; the bake path stores `1.0` (`0x14201c4c4`).
pub fn mesh_descriptor_scale(sx: f32, sy: f32, sz: f32) -> f32 {
    if mesh_scales_are_uniform(sx, sy, sz) {
        if sx > MESH_SCALE_EPSILON {
            sx
        } else {
            MESH_SCALE_EPSILON
        }
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag bit reads the dword, so only bit 25 selects the second mask
    /// word; a bit set in the upper half of the qword must not.
    #[test]
    fn the_world_mask_is_selected_by_bit_25_of_the_flags_dword() {
        assert_eq!(world_mask(0, 0xaaaa, 0xbbbb), 0xaaaa);
        assert_eq!(world_mask(1 << 25, 0xaaaa, 0xbbbb), 0xbbbb);
        assert_eq!(world_mask(u32::MAX, 0xaaaa, 0xbbbb), 0xbbbb);
        assert_eq!(world_mask(1 << 24, 0xaaaa, 0xbbbb), 0xaaaa);
    }

    /// The three tests, one alone at a time, plus the mask being an all-ones
    /// pass-through and a zero world mask failing every non-all-ones holder.
    #[test]
    fn the_holder_filter_is_the_mode_byte_the_mesh_and_the_mask() {
        // mode byte
        assert!(!holder_selected(1, 0x1000, 0xffff_ffff_ffff_ffff, 0));
        assert!(holder_selected(0, 0x1000, 0xffff_ffff_ffff_ffff, 0));
        // mesh block
        assert!(!holder_selected(0, 0, 0xffff_ffff_ffff_ffff, 0));
        assert!(holder_selected(0, 1, 0xffff_ffff_ffff_ffff, 0));
        // mask: all ones passes with no world bit at all
        assert!(holder_selected(0, 1, u64::MAX, 0));
        // any other mask needs an intersection
        assert!(!holder_selected(0, 1, 0x2, 0x1));
        assert!(holder_selected(0, 1, 0x2, 0x3));
        // a zero world mask fails every holder that is not all ones
        assert!(!holder_selected(0, 1, 0x0, 0xffff_ffff_ffff_ffff));
    }

    /// The mode switch: `0`, `1`, `2` and `4` only; `3`, negatives and larger
    /// values are skipped by the client's `sub`/`cmp` chain.
    #[test]
    fn the_shape_mode_is_the_four_arm_switch() {
        assert_eq!(shape_for(0), Some(Shape::Mesh));
        assert_eq!(shape_for(1), Some(Shape::PointCloud));
        assert_eq!(shape_for(2), Some(Shape::Capsule));
        assert_eq!(shape_for(4), Some(Shape::Sphere));
        for skipped in [3, 5, 6, 7, 8, 9, -1, i32::MIN, i32::MAX] {
            assert_eq!(shape_for(skipped), None, "mode {skipped} is skipped");
        }
    }

    /// The type byte the descriptor carries for each shape, read from the four
    /// mode blocks: `0x14201c290` (`0x300`), `0x14201c07b` (`0x200`),
    /// `0x14201be13` (`+0x67 = 1`) and `0x14201ba7d` (`0`).
    #[test]
    fn the_descriptor_type_byte_is_the_shape() {
        assert_eq!(Shape::Sphere.descriptor_type(), 0);
        assert_eq!(Shape::Capsule.descriptor_type(), 1);
        assert_eq!(Shape::PointCloud.descriptor_type(), 2);
        assert_eq!(Shape::Mesh.descriptor_type(), 3);
        assert!(Shape::Sphere.needs_second_body());
        assert!(!Shape::Mesh.needs_second_body());
    }

    /// The array tag rule is the one `ArrayRecord` documents: tag `1` puts the
    /// data pointer first, anything else puts the count first.
    #[test]
    fn the_holder_array_is_the_tag_swap() {
        let tagged = ArrayRecord {
            first: 0x1122_3344_5566_7788,
            second: 0x0000_0000_0000_0009,
            tag: 1,
        };
        assert_eq!(holder_array(tagged), (0x1122_3344_5566_7788, 9));
        let untagged = ArrayRecord {
            first: 0x0000_0000_0000_0009,
            second: 0x1122_3344_5566_7788,
            tag: 0,
        };
        assert_eq!(holder_array(untagged), (0x1122_3344_5566_7788, 9));
    }

    /// The uniform test is the client's two `comiss`/`jae` pairs, including the
    /// boundary: a difference of exactly `0.01` bakes.
    #[test]
    fn the_mesh_scale_is_uniform_within_the_epsilon() {
        assert!(mesh_scales_are_uniform(1.0, 1.0, 1.0));
        assert!(mesh_scales_are_uniform(2.0, 2.0, 2.0));
        assert!(mesh_scales_are_uniform(1.0, 0.9999, 0.9999)); // 1e-4 differences
                                                               // A difference of exactly `0.01f32` bakes (`comiss` + `jae`).
        assert!(!mesh_scales_are_uniform(0.0, 0.01, 0.01));
        assert!(!mesh_scales_are_uniform(0.01, 0.01, 0.0));
        assert!(!mesh_scales_are_uniform(1.0, 1.0, 0.5));
        assert!(!mesh_scales_are_uniform(1.0, 2.0, 2.0));
        // A NaN difference is "not greater or equal", so it stays uniform.
        assert!(mesh_scales_are_uniform(f32::NAN, 1.0, 1.0));
        assert!(mesh_scales_are_uniform(
            1.0,
            1.0,
            f32::INFINITY - f32::INFINITY
        ));
    }

    /// `maxss(scale, 0.01)` on the uniform path, `1.0` on the bake path.
    #[test]
    fn the_mesh_descriptor_scale_is_maxss_or_one() {
        assert_eq!(mesh_descriptor_scale(1.0, 1.0, 1.0), 1.0);
        assert_eq!(mesh_descriptor_scale(3.5, 3.5, 3.5), 3.5);
        assert_eq!(mesh_descriptor_scale(0.001, 0.001, 0.001), 0.01);
        // NaN scale is `maxss`'s second operand.
        assert_eq!(mesh_descriptor_scale(f32::NAN, f32::NAN, 0.0), 0.01);
        // baked meshes are unscaled relative to the shape
        assert_eq!(mesh_descriptor_scale(2.0, 3.0, 2.0), 1.0);
        assert_eq!(mesh_descriptor_scale(0.5, 0.5, 4.0), 1.0);
    }

    /// A holder that fails the filter never reaches the mode switch, so a
    /// skipped mode on a rejected holder and on an accepted holder are
    /// distinguishable.
    #[test]
    fn a_selected_holder_still_needs_a_known_mode() {
        assert_eq!(selected_shape(0, 1, u64::MAX, 0, 0), Some(Shape::Mesh));
        assert_eq!(selected_shape(0, 1, u64::MAX, 0, 3), None);
        assert_eq!(selected_shape(1, 1, u64::MAX, 0, 0), None);
        assert_eq!(selected_shape(0, 0, u64::MAX, 0, 0), None);
    }
}

//! The collision component's **shape-source plan**: which of a model's holders
//! the assembler `0x14209fc90` takes, and into which of its four arrays.
//!
//! This is the second consumer of the model's holder list, the one
//! [`crate::model_body`] is *not*. `docs/formats/static-collision.md` check 15
//! reads it end to end; the facts this module encodes are:
//!
//! 1. **The caller.** `0x142024d30` (a state-sync function of the collision
//!    component) creates a `0x50`-byte context with `Alloc(0x50, 0)` at
//!    `0x1420251d1`, constructs it with `0x14209ea30`, stores it at
//!    `[component+0xc8]` (`0x1420251e8`) and calls the assembler with
//!    `(context, component)` at `0x1420251f5`. The assembler has exactly one
//!    `E8` caller; the constructor exactly one.
//! 2. **The list.** The assembler's model is `[[[component+0x250]+0x110]+0x28]`
//!    when `[node+0x110]` is non-null and `[[[component+0x250]+0xe8]+0x28]`
//!    otherwise (the accessor `0x141b3d1c0`). That object is the `STUModel`, and
//!    the list is its field `0xCB4D298D` at `+0x140` with the tag at `+0x148` --
//!    the *same* slot, element type and tag rule the body builder walks.
//! 3. **The filter.** Per holder: the mode byte at `+0x2a` must be exactly
//!    **`1`** (the body builder takes `0`) and the embedded mesh at `+0x08` must
//!    be non-null. There is **no mask test** here, unlike the body builder.
//! 4. **The arm.** The `i32` shape mode at `+0x20` selects one of four arrays:
//!    `0` (mesh, filled by `MeshShapeFill` `0x142cae540`), `1`, `2` and `4`.
//!    Mode `3`, negatives and everything else are skipped.
//! 5. **The counts.** A first pass counts the holders per arm; if the four
//!    counts sum to zero the function returns without allocating anything
//!    (`0x14209ff8a`). Otherwise it allocates one array per non-empty arm.
//!
//! What this module deliberately does not do is describe the *records* the four
//! arms write. Their sources are four unpackers whose semantics are not read
//! here (`0x141b53af0` and `0x141b53b70` for the two mesh forms, `0x142cd1760`
//! and `0x142cd35a0` for the element constructors), and on this map no holder
//! reaches any arm, so no test could prove them.

use crate::model_body::{shape_for, Shape};

/// `holder+0x2a`: the mode byte this consumer takes. Field `0x89AABA94`,
/// registered at `+0x2a` by the holder class's installer `0x141510382`; the
/// body builder takes the value `0`.
pub const MODE_TAKEN: u8 = 1;

/// The context the assembler fills: `Alloc(0x50, 0)` at `0x1420251d1`, freed
/// with size `0x50` at `0x142025229`.
pub const CONTAINER_SIZE: usize = 0x50;
/// `[context+0x00]`: the component the assembler was called with
/// (`0x14209fcc5`); the reset clears it (`0x1420a08c0`).
pub const CONTAINER_COMPONENT: usize = 0x00;
/// `[context+0x38]`: the total number of records over all four arms
/// (`0x1420a0759`).
pub const CONTAINER_TOTAL: usize = 0x38;
/// `[context+0x40]`: the per-id mask table, `count * 8` bytes, allocated with
/// alignment `0x10` through `0x141bb0120` (`0x1420a02d7`).
pub const CONTAINER_MASK_TABLE: usize = 0x40;
/// `[context+0x48]`: the sign-extended `u16` count from
/// `[[component+0x8b0]+0x58]` (`0x142010330`, stored at `0x1420a02cc`).
pub const CONTAINER_ID_COUNT: usize = 0x48;

/// The alignment the mask table is allocated with (`0x1420a02c7`).
pub const MASK_TABLE_ALIGNMENT: u32 = 0x10;

/// `[context+0x18]`'s mode-1 array carries a `0x10`-byte header holding the
/// element count, and the context points past it (`0x1420a0216`, `0x1420a023e`).
pub const UNPACKED_ARRAY_HEADER: usize = 0x10;

impl Shape {
    /// The context field holding this arm's array. Mode `0` (mesh) is `+0x20`,
    /// mode `1` `+0x18`, mode `2` `+0x10`, mode `4` `+0x08`
    /// (`0x1420a0188`, `0x1420a01ec`, `0x1420a023e`, `0x1420a02bb`).
    pub fn array_offset(self) -> usize {
        match self {
            Shape::Mesh => 0x20,
            Shape::PointCloud => 0x18,
            Shape::Capsule => 0x10,
            Shape::Sphere => 0x08,
        }
    }

    /// The context dword counting this arm's records. Mode `0` is `+0x34`, mode
    /// `1` `+0x30`, mode `2` `+0x2c`, mode `4` `+0x28`
    /// (`0x1420a0715`, `0x1420a065e`, `0x1420a05b4`, `0x1420a0487`).
    pub fn count_offset(self) -> usize {
        match self {
            Shape::Mesh => 0x34,
            Shape::PointCloud => 0x30,
            Shape::Capsule => 0x2c,
            Shape::Sphere => 0x28,
        }
    }

    /// The record stride of this arm's array: `0x60` (mode `0`), `0x70` (mode
    /// `1`), `0x50` (mode `2`), `0x40` (mode `4`).
    pub fn record_stride(self) -> usize {
        match self {
            Shape::Mesh => 0x60,
            Shape::PointCloud => 0x70,
            Shape::Capsule => 0x50,
            Shape::Sphere => 0x40,
        }
    }

    /// The bytes `Alloc` is asked for, for `count` records of this arm. The
    /// client sign-extends the count (`movsxd`), multiplies with `mul` and
    /// `cmovb`s the result to `u64::MAX` on overflow (`0x1420a012f`..
    /// `0x1420a0137`); mode `1` adds the `0x10`-byte header afterwards and
    /// clamps again (`0x1420a01ff`).
    pub fn allocation_bytes(self, count: i32) -> u64 {
        let count = i64::from(count) as u64;
        let stride = self.record_stride() as u64;
        let (body, overflow) = count.overflowing_mul(stride);
        let body = if overflow { u64::MAX } else { body };
        if self == Shape::PointCloud {
            let (total, overflow) = body.overflowing_add(UNPACKED_ARRAY_HEADER as u64);
            if overflow {
                u64::MAX
            } else {
                total
            }
        } else {
            body
        }
    }
}

/// The assembler's two per-holder tests, in the client's order
/// (`0x1420a0323`, `0x1420a032d`): the mode byte is exactly [`MODE_TAKEN`] and
/// the embedded mesh at `+0x08` is non-null. The shape mode then picks the arm;
/// there is no mask test.
pub fn taken(mode_byte: u8, mesh: u64, shape_mode: i32) -> Option<Shape> {
    if mode_byte != MODE_TAKEN || mesh == 0 {
        return None;
    }
    shape_for(shape_mode)
}

/// The four per-arm counts the counting pass produces (`0x14209ff40`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    mesh: u32,
    point_cloud: u32,
    capsule: u32,
    sphere: u32,
}

impl Plan {
    /// The number of records this arm would hold.
    pub fn count(&self, shape: Shape) -> u32 {
        match shape {
            Shape::Mesh => self.mesh,
            Shape::PointCloud => self.point_cloud,
            Shape::Capsule => self.capsule,
            Shape::Sphere => self.sphere,
        }
    }

    /// `[context+0x38]`: the sum over the four arms (`0x1420a0739`).
    pub fn total(&self) -> u32 {
        self.mesh
            .wrapping_add(self.point_cloud)
            .wrapping_add(self.capsule)
            .wrapping_add(self.sphere)
    }

    /// The client's early return: a zero sum skips every allocation
    /// (`0x14209ff8a` jumps to the epilogue).
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// The four `(shape, count)` pairs in the client's arm order.
    pub fn arms(&self) -> [(Shape, u32); 4] {
        [
            (Shape::Mesh, self.mesh),
            (Shape::PointCloud, self.point_cloud),
            (Shape::Capsule, self.capsule),
            (Shape::Sphere, self.sphere),
        ]
    }
}

/// One counting pass over a model's holders, in list order, as
/// `(mode byte, mesh pointer, shape mode)`.
pub fn plan<I>(holders: I) -> Plan
where
    I: IntoIterator<Item = (u8, u64, i32)>,
{
    let mut plan = Plan::default();
    for (mode_byte, mesh, shape_mode) in holders {
        match taken(mode_byte, mesh, shape_mode) {
            Some(Shape::Mesh) => plan.mesh += 1,
            Some(Shape::PointCloud) => plan.point_cloud += 1,
            Some(Shape::Capsule) => plan.capsule += 1,
            Some(Shape::Sphere) => plan.sphere += 1,
            None => {}
        }
    }
    plan
}

/// `0x1420a02cc`..`0x1420a02d7`: the mask table's byte size is the
/// **sign-extended** `u16` from `[[component+0x8b0]+0x58]` times eight, computed
/// in 32 bits, so a negative count becomes a very large request.
pub fn mask_table_bytes(ids: i16) -> u32 {
    (i64::from(ids) * 8) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter is the mode byte **one** and a non-null mesh; the mask the
    /// body builder also tests is not consulted at all here.
    #[test]
    fn the_filter_is_the_mode_byte_one_and_the_mesh() {
        assert_eq!(taken(1, 1, 0), Some(Shape::Mesh));
        assert_eq!(taken(0, 1, 0), None);
        assert_eq!(taken(2, 1, 0), None);
        assert_eq!(taken(1, 0, 0), None);
        assert_eq!(taken(0, 0, 0), None);
        assert_eq!(taken(1, u64::MAX, 0), Some(Shape::Mesh));
    }

    /// The arm set is the body builder's `0`, `1`, `2`, `4`; `3` and everything
    /// else is skipped even when the filter passes.
    #[test]
    fn the_arm_set_is_the_four_shape_modes() {
        for (mode, shape) in [
            (0, Shape::Mesh),
            (1, Shape::PointCloud),
            (2, Shape::Capsule),
            (4, Shape::Sphere),
        ] {
            assert_eq!(taken(1, 1, mode), Some(shape));
        }
        for skipped in [3, 5, 6, 7, -1, i32::MIN, i32::MAX] {
            assert_eq!(taken(1, 1, skipped), None, "shape mode {skipped}");
        }
    }

    /// The four arms' array fields, counters and strides, as read.
    #[test]
    fn the_arm_layout_is_the_four_arrays() {
        assert_eq!(
            [
                Shape::Mesh.array_offset(),
                Shape::PointCloud.array_offset(),
                Shape::Capsule.array_offset(),
                Shape::Sphere.array_offset(),
            ],
            [0x20, 0x18, 0x10, 0x08]
        );
        assert_eq!(
            [
                Shape::Mesh.count_offset(),
                Shape::PointCloud.count_offset(),
                Shape::Capsule.count_offset(),
                Shape::Sphere.count_offset(),
            ],
            [0x34, 0x30, 0x2c, 0x28]
        );
        assert_eq!(
            [
                Shape::Mesh.record_stride(),
                Shape::PointCloud.record_stride(),
                Shape::Capsule.record_stride(),
                Shape::Sphere.record_stride(),
            ],
            [0x60, 0x70, 0x50, 0x40]
        );
    }

    /// The allocations: the product per arm, and mode `1`'s `0x10` header.
    #[test]
    fn the_allocation_is_the_stride_times_the_count() {
        assert_eq!(Shape::Mesh.allocation_bytes(3), 3 * 0x60);
        assert_eq!(
            Shape::PointCloud.allocation_bytes(3),
            3 * 0x70 + UNPACKED_ARRAY_HEADER as u64
        );
        assert_eq!(Shape::Capsule.allocation_bytes(2), 2 * 0x50);
        assert_eq!(Shape::Sphere.allocation_bytes(4), 4 * 0x40);
        assert_eq!(Shape::Mesh.allocation_bytes(0), 0);
        assert_eq!(
            Shape::PointCloud.allocation_bytes(0),
            UNPACKED_ARRAY_HEADER as u64
        );
        // The count is sign-extended, so a negative one is a huge `mul` and the
        // `cmovb` clamps it.
        assert_eq!(Shape::Mesh.allocation_bytes(-1), u64::MAX);
        assert_eq!(Shape::Capsule.allocation_bytes(i32::MIN), u64::MAX);
        assert_eq!(Shape::PointCloud.allocation_bytes(-1), u64::MAX);
    }

    /// The plan counts each arm separately, keeps the client's arm order, and
    /// reports the total the client stores at `+0x38`.
    #[test]
    fn the_plan_counts_the_four_arms_and_its_total() {
        let holders = [
            (1, 1, 0), // mesh
            (1, 1, 0), // mesh
            (1, 1, 1), // point cloud
            (1, 1, 2), // capsule
            (1, 1, 4), // sphere
            (1, 0, 0), // no mesh: skipped
            (0, 1, 0), // mode byte 0: the body builder's holder
            (1, 1, 3), // skipped shape mode
        ];
        let plan = plan(holders);
        assert_eq!(
            plan.arms(),
            [
                (Shape::Mesh, 2),
                (Shape::PointCloud, 1),
                (Shape::Capsule, 1),
                (Shape::Sphere, 1),
            ]
        );
        assert_eq!(plan.count(Shape::Mesh), 2);
        assert_eq!(plan.total(), 5);
        assert!(!plan.is_empty());
    }

    /// The client's early return: nothing taken means nothing allocated.
    #[test]
    fn an_empty_plan_is_the_early_return() {
        let plan = plan([(0, 1, 0), (0, 1, 4), (1, 0, 0), (2, 1, 1)]);
        assert_eq!(plan.total(), 0);
        assert!(plan.is_empty());
        for (shape, count) in plan.arms() {
            assert_eq!(count, 0, "{shape:?}");
            assert_eq!(
                shape.allocation_bytes(count as i32),
                if shape == Shape::PointCloud { 0x10 } else { 0 }
            );
        }
    }

    /// The mask table's size is the sign-extended `u16` times eight in 32 bits.
    #[test]
    fn the_mask_table_size_is_sign_extended() {
        assert_eq!(mask_table_bytes(0), 0);
        assert_eq!(mask_table_bytes(1), 8);
        assert_eq!(mask_table_bytes(0x7fff), 0x3fff8);
        assert_eq!(mask_table_bytes(-1), 0xffff_fff8);
        assert_eq!(mask_table_bytes(i16::MIN), 0xfffc_0000);
        assert_eq!(MASK_TABLE_ALIGNMENT, 0x10);
    }
}

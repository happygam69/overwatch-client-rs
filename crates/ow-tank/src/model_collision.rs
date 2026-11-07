//! The model's collision descriptor `C` — the class behind
//! `[[[node+0x38]+0x40]+0x48]` — and the two read expressions the client uses
//! on it.
//!
//! `C` is a **registered class instance**, identified by type rather than by a
//! displacement: type `0x564B1018`, size `0x198`, align 8, descriptor
//! `0x143e74830`, field table `0x143e74790`, construct-in-place hook
//! `0x1413354b0`, factory `0x141335610`, vtable `0x1431dd118`. The evidence is
//! in `docs/formats/model-collision.md`; in one line, the client's own
//! registration gives the size, its constructor installs the three inline
//! sub-object vtables the class declares, and the class's own per-field
//! dispatch functions compare 22 of the class's declared field hashes.
//!
//! What lives here is only what the client's code was read to do:
//!
//! - [`ArrayRecord`]: `GeometryBuild` (`0x14213c440`) turns the 16-byte
//!   array slot at `C+0x98` plus the tag at `C+0xa0` into a point array and a
//!   count. The same expression is used twice in that function — once for the
//!   "is there anything here" guard and once for the ring builder's arguments.
//! - [`flag_mask`]: `0x141b53c40` folds one of `C`'s two 14-byte flag records
//!   (`C+0x178`, `C+0x186`) into a `u16`.
//!
//! Nothing here decodes an asset: the instance has to come from a model's STU
//! payload, which is `ow-stu`'s job. The offsets below are the ones the client
//! reads, with the field each one is in the class's declaration.

/// The registered type hash of `C`. `desc+0xb8` of `0x143e74830`.
pub const CLASS_HASH: u32 = 0x564b1018;
/// The instance size the client registers: `desc+0xa0` = `0x198`.
pub const CLASS_SIZE: u32 = 0x198;
/// The class's vtable, written by the constructor at `0x1413354b0`.
pub const CLASS_VTABLE: u64 = 0x1431dd118;
/// The type descriptor the registration fills.
pub const CLASS_DESCRIPTOR: u64 = 0x143e74830;
/// The field table pointer stored at `desc+0x08`.
pub const CLASS_FIELD_TABLE: u64 = 0x143e74790;
/// `desc+0x60`: construct an instance in place over payload bytes.
pub const CLASS_CONSTRUCT: u64 = 0x1413354b0;
/// `desc+0x58`: allocate `0x198` bytes and construct.
pub const CLASS_FACTORY: u64 = 0x141335610;

/// `C+0x98`: the first qword of an inline sub-object's 16-byte array slot.
/// After loading it is a pointer to a 16-byte record that holds the array's
/// count and data pointer, in one of two orders.
pub const OFF_ARRAY_SLOT: usize = 0x98;
/// `C+0xa0`: the slot's second qword. `1` selects the `{data, count}` order.
pub const OFF_ARRAY_TAG: usize = 0xa0;
/// `C+0x178`: first 14-byte flag record (`STU_933AD0A9`).
pub const OFF_FLAGS_A: usize = 0x178;
/// `C+0x186`: second 14-byte flag record, the same type.
pub const OFF_FLAGS_B: usize = 0x186;
/// `C+0x194` and `C+0x195`: the two bytes `GeometryCopyParams` copies.
pub const OFF_BYTE_A: usize = 0x194;
pub const OFF_BYTE_B: usize = 0x195;

/// The 16-byte record `C+0x98` points at, as the two qwords the client reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayRecord {
    /// The record's first qword — `points` when [`Self::tag`] is `1`, else
    /// `count`.
    pub first: u64,
    /// The record's second qword.
    pub second: u64,
    /// The tag at `C+0xa0`, compared against exactly `1`.
    pub tag: u64,
}

impl ArrayRecord {
    /// The client's expression, operation for operation: when the tag is `1`
    /// the first qword is the data pointer and the second qword's low `u32` is
    /// the count; otherwise the first qword's low `u32` is the count and the
    /// second qword is the data pointer.
    pub fn points_and_count(&self) -> (u64, u32) {
        if self.tag == 1 {
            (self.first, self.second as u32)
        } else {
            (self.second, self.first as u32)
        }
    }

    /// `GeometryBuild` refuses the build when the count reads zero, before it
    /// looks at the data pointer at all.
    pub fn is_empty(&self) -> bool {
        self.points_and_count().1 == 0
    }
}

/// The fold `0x141b53c40` applies to one 14-byte flag record: `h = (h << 1) +
/// byte` over the bytes in the order the client's compiler emitted them,
/// wrapping to 16 bits. The order is not the byte order: the record's byte
/// indices are folded as 6, 2, 13, 12, 8, 1, 3, 5, 4, 11, 10, 0, 9, 7.
pub fn flag_mask(record: &[u8; 14]) -> u16 {
    const ORDER: [usize; 14] = [6, 2, 13, 12, 8, 1, 3, 5, 4, 11, 10, 0, 9, 7];
    let mut acc: u16 = 0;
    for i in ORDER {
        acc = acc.wrapping_shl(1).wrapping_add(record[i] as u16);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tag picks which qword is which; all four field values are distinct
    /// bit patterns so a swapped read cannot pass.
    #[test]
    fn the_array_record_is_read_the_way_geometry_build_reads_it() {
        let record = ArrayRecord {
            first: 0x0000_0000_1122_3344,
            second: 0x0000_0003_5566_7788,
            tag: 0,
        };
        assert_eq!(
            record.points_and_count(),
            (0x0000_0003_5566_7788, 0x1122_3344)
        );

        let record = ArrayRecord {
            first: 0x0000_0000_1122_3344,
            second: 0x0000_0003_5566_7788,
            tag: 1,
        };
        assert_eq!(
            record.points_and_count(),
            (0x0000_0000_1122_3344, 0x5566_7788)
        );
    }

    /// Only exactly `1` selects the swapped order — the client compares the
    /// whole qword, not its low byte.
    #[test]
    fn the_tag_is_a_whole_qword_test() {
        for tag in [0u64, 2, 0x100, u64::MAX] {
            let record = ArrayRecord {
                first: 7,
                second: 9,
                tag,
            };
            assert_eq!(record.points_and_count(), (9, 7));
        }
        let record = ArrayRecord {
            first: 7,
            second: 9,
            tag: 1,
        };
        assert_eq!(record.points_and_count(), (7, 9));
    }

    /// A zero count rejects the build before the data pointer is used, in both
    /// orders.
    #[test]
    fn a_zero_count_is_refused() {
        assert!(ArrayRecord {
            first: 0,
            second: 0x1000,
            tag: 0
        }
        .is_empty());
        assert!(ArrayRecord {
            first: 0x1000,
            second: 0,
            tag: 1
        }
        .is_empty());
        assert!(!ArrayRecord {
            first: 3,
            second: 0x1000,
            tag: 0
        }
        .is_empty());
    }

    /// The fold: the order is the client's, so a record of fourteen ones gives
    /// `0x3fff` and clearing byte 6 (the first folded, so the heaviest bit)
    /// clears `0x2000`.
    #[test]
    fn the_flag_fold_uses_the_client_byte_order() {
        let ones = [1u8; 14];
        assert_eq!(flag_mask(&ones), 0x3fff);

        let mut with_gap = [1u8; 14];
        with_gap[6] = 0;
        assert_eq!(flag_mask(&with_gap), 0x3fff - 0x2000);

        let mut shifted = [0u8; 14];
        shifted[7] = 1; // the last byte folded, so the lowest bit
        assert_eq!(flag_mask(&shifted), 1);
        shifted[7] = 0;
        shifted[6] = 1; // the first byte folded, so the highest bit
        assert_eq!(flag_mask(&shifted), 1 << 13);
    }

    /// The offsets and the class identity are the client's own registration;
    /// a change here is a change to a client value, not a formatting choice.
    #[test]
    fn the_class_identity_is_the_registered_one() {
        assert_eq!(CLASS_HASH, 0x564b1018);
        assert_eq!(CLASS_SIZE, 0x198);
        assert_eq!(CLASS_VTABLE, 0x1431dd118);
        assert_eq!(OFF_ARRAY_SLOT + 8, OFF_ARRAY_TAG);
        assert_eq!(OFF_FLAGS_A + 14, OFF_FLAGS_B);
        assert_eq!(OFF_FLAGS_B + 14, OFF_BYTE_A);
        assert_eq!(OFF_BYTE_A + 1, OFF_BYTE_B);
        assert!((OFF_BYTE_B as u32) < CLASS_SIZE);
    }
}

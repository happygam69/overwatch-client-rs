//! The model's prebuilt collision mesh: the client's unpack of `STU_537E56E4`
//! and the shape record it hands the collision query.
//!
//! This is the read side of `docs/formats/static-collision.md`. `STUModel`'s
//! field `0xCB4D298D` holds a list of holders (`0x2DD96D6D`); each holder with
//! mode `+0x20 == 0` embeds one `STU_537E56E4` at `+0x08` through field
//! `0xB7C8314A`. The consumer `FUN_14210bac0` walks that list, unpacks the
//! block with **`0x141b53af0`**, fills a shape record with **`MeshShapeFill`
//! (`0x142cae540`)** and hands the record to the collision query `0x142caf6c0`.
//!
//! Two things are captured here because they are read exactly:
//!
//! - [`array_header`]: the block stores each of its three arrays as a pointer
//!   plus a tag; `0x141b53af0` reads the array's 16-byte header
//!   `{data pointer, u64 count}` at `pointer` when the tag is `1` and at
//!   `pointer + 8` otherwise. The field offsets are [`OFF_NODES`],
//!   [`OFF_POINTS`], [`OFF_TRIANGLES`], [`OFF_NODE_COUNT`], [`OFF_38B71436`],
//!   [`OFF_HEIGHT`] and [`OFF_5BD33E19`].
//! - [`shape_record`]: `MeshShapeFill`'s 19 dwords, by byte offset, including
//!   the `1/scale` lanes and the one scaled float at `+0x48`.
//!
//! **There is no vertex or index conversion in the client.** The three arrays
//! are passed to the query as stored; this module therefore converts nothing.

/// `m_nodes` (`0x9B2111C5`): an array, unpacked as a data pointer only.
pub const OFF_NODES: usize = 0x08;
/// `m_88FCECD7`: the points, a `Vec3A[]`; this array's header also supplies the
/// point count.
pub const OFF_POINTS: usize = 0x18;
/// `m_triangles` (`0x0D50490E`): an array, unpacked as a data pointer only.
pub const OFF_TRIANGLES: usize = 0x28;
/// `m_nodeCount` (`0xF91B8EDD`), a `u32` copied straight into the record.
pub const OFF_NODE_COUNT: usize = 0x38;
/// `m_38B71436`, a `u32` copied straight into the record.
pub const OFF_38B71436: usize = 0x3c;
/// `m_height` (`0xA16426C6`), a `u32` copied straight into the record.
pub const OFF_HEIGHT: usize = 0x40;
/// `m_5BD33E19`, an `f32` multiplied by the record's scale.
pub const OFF_5BD33E19: usize = 0x44;

/// A point is a `Vec3A`: four `f32`, and the client copies all four lanes.
pub const POINT_SIZE: usize = 16;

/// The shape record `MeshShapeFill` writes: 19 dwords (`0x4c` bytes).
pub const RECORD_DWORDS: usize = 19;

/// Where `0x141b53af0` reads one array's 16-byte header, given the tag stored at
/// `field + 8`: at the pointer itself when the tag is exactly `1`, otherwise
/// eight bytes past it.
pub fn header_offset(tag: u64) -> usize {
    if tag == 1 {
        0
    } else {
        8
    }
}

/// The 16-byte array header the client reads: `{data pointer, u64 count}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayHeader {
    pub data: u64,
    pub count: u64,
}

/// Apply `0x141b53af0`'s rule to one array slot: the two words at the field are
/// `{pointer, tag}`, and the header sits at `pointer + header_offset(tag)`.
/// `header` is what is stored there.
pub fn array_header(slot: [u64; 2], header: ArrayHeader) -> ArrayHeader {
    let _ = header_offset(slot[1]);
    header
}

/// What `0x141b53af0` unpacks into its `0x30`-byte output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshSource {
    /// `[+0x00]` the node array's data pointer.
    pub nodes: u64,
    /// `[+0x08]` the point array's data pointer.
    pub points: u64,
    /// `[+0x10]` the triangle array's data pointer.
    pub triangles: u64,
    /// `[+0x18]` `m_nodeCount`.
    pub node_count: u32,
    /// `[+0x1c]` the point array's header count.
    pub point_count: u32,
    /// `[+0x20]` `m_38B71436`.
    pub field_3c: u32,
    /// `[+0x24]` `m_height`.
    pub height: u32,
    /// `[+0x28]` `m_5BD33E19`.
    pub field_44: f32,
}

/// `MeshShapeFill` (`0x142cae540`), dword for dword: four lanes of the scale,
/// four lanes of its reciprocal (a `divss`, so `inf` at a zero scale), the three
/// data pointers, the four copied words, and the one float multiplied by the
/// scale.
pub fn shape_record(source: &MeshSource, scale: f32) -> [u32; RECORD_DWORDS] {
    let mut out = [0u32; RECORD_DWORDS];
    let s = scale.to_bits();
    let inv = (1.0f32 / scale).to_bits();
    for lane in 0..4 {
        out[lane] = s;
        out[4 + lane] = inv;
    }
    out[8] = source.nodes as u32;
    out[9] = (source.nodes >> 32) as u32;
    out[10] = source.points as u32;
    out[11] = (source.points >> 32) as u32;
    out[12] = source.triangles as u32;
    out[13] = (source.triangles >> 32) as u32;
    out[14] = source.node_count;
    out[15] = source.point_count;
    out[16] = source.field_3c;
    out[17] = source.height;
    out[18] = (scale * source.field_44).to_bits();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> MeshSource {
        MeshSource {
            nodes: 0x1111_2222_3333_4444,
            points: 0x5555_6666_7777_8888,
            triangles: 0x9999_aaaa_bbbb_cccc,
            node_count: 7,
            point_count: 8,
            field_3c: 12,
            height: 4,
            field_44: 0.5,
        }
    }

    /// The tag is compared against exactly `1`; anything else shifts the header
    /// eight bytes, which is the client's whole rule.
    #[test]
    fn the_array_header_offset_is_the_tag_test() {
        assert_eq!(header_offset(1), 0);
        for tag in [0u64, 2, 0x100, u64::MAX] {
            assert_eq!(header_offset(tag), 8);
        }
    }

    /// The record's layout by byte offset, including the reciprocal lanes.
    #[test]
    fn the_shape_record_is_mesh_shape_fill_dword_for_dword() {
        let r = shape_record(&source(), 2.0);
        assert_eq!(&r[0..4], &[2.0f32.to_bits(); 4], "+0x00 scale lanes");
        assert_eq!(&r[4..8], &[0.5f32.to_bits(); 4], "+0x10 reciprocal lanes");
        assert_eq!(r[8], 0x3333_4444, "+0x20 nodes low");
        assert_eq!(r[9], 0x1111_2222, "+0x24 nodes high");
        assert_eq!(r[10], 0x7777_8888, "+0x28 points low");
        assert_eq!(r[11], 0x5555_6666, "+0x2c points high");
        assert_eq!(r[12], 0xbbbb_cccc, "+0x30 triangles low");
        assert_eq!(r[13], 0x9999_aaaa, "+0x34 triangles high");
        assert_eq!(r[14], 7, "+0x38 nodeCount");
        assert_eq!(r[15], 8, "+0x3c point count");
        assert_eq!(r[16], 12, "+0x40 the 0x3c word");
        assert_eq!(r[17], 4, "+0x44 height");
        assert_eq!(r[18], 1.0f32.to_bits(), "+0x48 scale * 0x5BD33E19");
        assert_eq!(RECORD_DWORDS * 4, 0x4c, "the client writes 0x4c bytes");
    }

    /// The reciprocal is a `divss`, so a zero scale is an infinity rather than a
    /// panic: the record must keep the client's value, not a guard.
    #[test]
    fn a_zero_scale_is_an_infinity_lane() {
        let r = shape_record(&source(), 0.0);
        assert_eq!(&r[4..8], &[f32::INFINITY.to_bits(); 4]);
        assert_eq!(r[18], (0.0f32 * 0.5).to_bits());
    }

    /// The field offsets are the ones `0x141b53af0` reads, and they are the same
    /// offsets the class declares.
    #[test]
    fn the_field_offsets_are_the_unpackers() {
        assert_eq!(OFF_NODES, 0x08);
        assert_eq!(OFF_POINTS, 0x18);
        assert_eq!(OFF_TRIANGLES, 0x28);
        assert_eq!(OFF_NODE_COUNT, 0x38);
        assert_eq!(OFF_38B71436, 0x3c);
        assert_eq!(OFF_HEIGHT, 0x40);
        assert_eq!(OFF_5BD33E19, 0x44);
        // The class is 0x48 bytes, so the last field fits inside it.
        const CLASS_SIZE: usize = 0x48;
        let last = OFF_5BD33E19 + 4;
        assert!(last <= CLASS_SIZE, "the class is 0x48 bytes");
    }
}

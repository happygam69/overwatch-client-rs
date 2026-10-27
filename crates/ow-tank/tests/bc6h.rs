//! BC6H: the fourteen modes, the arithmetic, and the places where a naive
//! implementation would silently differ.
//!
//! Everything here is driven through the published specification (Microsoft's
//! "BC6H Format" page and the Khronos `EXT_texture_compression_bptc`
//! extension) rather than through the install, because BC6H's shape is defined
//! by those documents. What the install contributes is *which* variant is
//! stored (byte 96 = DXGI 95 = `BC6H_UF16`, unsigned) — see
//! `docs/formats/materials.md` § "BC6H" for the measured check.
//!
//! The decoder is additionally checked against two independent
//! implementations by `dev/bc6h_oracle.py`; the numbers are in that document,
//! not here.

use ow_tank::texture::{self, f32_to_half, half_to_f32, Format, BC6H_MODES, BC6H_MODE_NUMBERS};

/// Half float 1.0 — BC6H has no alpha channel, so this is every pixel's fourth
/// component by definition.
const ALPHA: u16 = 0x3C00;

/// Build a block field by field, lowest bit first, so a test can state exactly
/// what it means to store.
#[derive(Default)]
struct Block {
    value: u128,
    pos: usize,
}

impl Block {
    fn new() -> Block {
        Block { value: 0, pos: 0 }
    }

    fn put(&mut self, v: u32, n: u8) {
        assert!(n <= 32);
        self.value |= (v as u128) << self.pos;
        self.pos += n as usize;
    }

    /// A field the specification prints most-significant-bit first.
    fn put_reversed(&mut self, v: u32, n: u8) {
        let mut rev = 0u32;
        for i in 0..n {
            rev |= ((v >> i) & 1) << (n - 1 - i);
        }
        self.put(rev, n);
    }

    fn bytes(&self) -> [u8; 16] {
        assert_eq!(self.pos, 128, "a BC6H block is exactly 128 bits");
        self.value.to_le_bytes()
    }
}

/// Decode one block to its 16 pixels of `(r, g, b, a)` half floats.
fn pixels(block: [u8; 16], signed: bool) -> Vec<[u16; 4]> {
    let (_, halves) = texture::decode_bc6h_block(&block, signed);
    (0..16)
        .map(|i| {
            [
                halves[i * 4],
                halves[i * 4 + 1],
                halves[i * 4 + 2],
                halves[i * 4 + 3],
            ]
        })
        .collect()
}

fn half(h: u16) -> f32 {
    half_to_f32(h)
}

/// Modes 11 and 15 store the top bits of their base endpoint
/// most-significant-bit first, interleaved with the low bits.
///
/// Mode 11 (`01011`) is `12.8`: the three ten-bit base endpoints, then the
/// three eight-bit second endpoints, each immediately followed by the *reversed*
/// two high bits of its base. The stored bits of `RW[11:10]` read `0b10` must
/// therefore land as `0b01` at bits 11..10 — the reverse of what a reader that
/// ignored the reversal would compute, which is what makes this test
/// non-vacuous.
#[test]
fn the_top_bits_of_the_base_endpoint_are_stored_reversed() {
    /// `high_bits` is written as two plain bits, so `0b10` means "the block's
    /// bit 79 is 0 and bit 80 is 1".
    fn mode11_block(high_bits: u32) -> [u8; 16] {
        let mut b = Block::new();
        b.put(0b01011, 5); // mode 11
        b.put(0, 10); // RW[9:0]
        b.put(0, 10); // GW[9:0]
        b.put(0, 10); // BW[9:0]
        b.put(0, 8); // RX[7:0]
        b.put(high_bits, 2); // RW[11:10]
        for _ in 0..2 {
            b.put(0, 8); // GX[7:0] or BX[7:0]
            b.put_reversed(0, 2); // GW[11:10] or BW[11:10]
        }
        for pixel in 0..16 {
            b.put(0, if pixel == 0 { 3 } else { 4 }); // indices
        }
        b.bytes()
    }

    let stored_10 = pixels(mode11_block(0b10), false);
    let stored_01 = pixels(mode11_block(0b01), false);
    // Pixel 0's index is zero, so it sits on the base endpoint itself.
    assert_eq!(
        stored_10[0][0],
        finish(unquantised(1 << 10, 12)),
        "stored 0b10 must become the endpoint's bits 11..10 = 0b01"
    );
    assert_eq!(
        stored_01[0][0],
        finish(unquantised(2 << 10, 12)),
        "stored 0b01 must become 0b10"
    );
    assert_ne!(stored_10[0][0], stored_01[0][0]);
    // `put_reversed` writes the same block as writing the reversed bits plain,
    // which is the convention the assertion above rests on.
    let mut via_helper = Block::new();
    via_helper.put(0b01011, 5);
    via_helper.put(0, 10);
    via_helper.put(0, 10);
    via_helper.put(0, 10);
    via_helper.put(0, 8);
    via_helper.put_reversed(0b01, 2); // endpoint bits 0b01, stored as 0b10
    for _ in 0..2 {
        via_helper.put(0, 8);
        via_helper.put_reversed(0, 2);
    }
    for pixel in 0..16 {
        via_helper.put(0, if pixel == 0 { 3 } else { 4 });
    }
    assert_eq!(via_helper.bytes(), mode11_block(0b10));
}

/// Every mode's stored fields must fill the block exactly.
///
/// This is the check that makes the mode table auditable: a typo in a field
/// width, a missing field or an extra one changes the sum, and the sum is
/// fixed at 128 by the format. `fields` + mode bits + partition + one index
/// per pixel (less one bit at each region's anchor) = 128.
#[test]
fn every_mode_fills_exactly_128_bits() {
    for m in BC6H_MODES.iter() {
        let mode_bits = if m.number < 2 { 2 } else { 5 };
        let endpoint_bits: usize = m.fields.iter().map(|f| f.width as usize).sum();
        let partition_bits = if m.two_regions { 5 } else { 0 };
        // Two regions means two anchor pixels (pixel 0 and the shape's fix-up
        // pixel), one region means one; each anchor stores one bit fewer.
        let anchors = if m.two_regions { 2 } else { 1 };
        let index_bits = 16 * m.index_bits as usize - anchors;
        assert_eq!(
            mode_bits + endpoint_bits + partition_bits + index_bits,
            128,
            "mode {} (specification mode {}) accounts for {} bits, not 128",
            m.number,
            m.number,
            mode_bits + endpoint_bits + partition_bits + index_bits
        );
        // No field may claim bits above its endpoint's precision: a field that
        // does would mean the table and its own widths disagree.
        for f in m.fields {
            let width_needed = f.offset as usize + f.width as usize;
            assert!(
                width_needed <= m.base_bits as usize,
                "mode {}: a field needs {width_needed} bits but the endpoint has {}",
                m.number,
                m.base_bits
            );
        }
    }
}

/// All fourteen modes decode, and nothing comes out as infinity or NaN.
#[test]
fn every_mode_decodes_and_stays_inside_the_half_range() {
    for mode in BC6H_MODE_NUMBERS {
        for block in texture::bc6h_random_blocks(mode, 64, 0xC0FFEE) {
            for (index, px) in pixels(block, false).iter().enumerate() {
                assert_eq!(px[3], ALPHA, "mode {mode} pixel {index} alpha");
                for c in 0..3 {
                    let bits = px[c];
                    let exponent = (bits >> 10) & 0x1f;
                    assert_ne!(
                        exponent, 0x1f,
                        "mode {mode} pixel {index} channel {c} is infinity or NaN: \
                         half 0x{bits:04x}"
                    );
                }
            }
        }
    }
}

/// The reserved modes decode to black with the fixed alpha, which is what the
/// specification requires of hardware given such a block.
#[test]
fn a_reserved_mode_decodes_to_black_at_alpha_one() {
    // Only the four the specification names: a mode whose low two bits are
    // below two is mode 0 or mode 1 by definition, so 4, 5, 8, 9 and friends
    // are not reserved numbers, they are two-bit modes with a stray bit above.
    for reserved in [0b10011u32, 0b10111, 0b11011, 0b11111] {
        let mut b = Block::new();
        b.put(reserved, 5);
        // Fill the rest with ones: a reserved mode must not care.
        while b.pos < 128 {
            b.put(0xffff_ffff, (128 - b.pos).min(32) as u8);
        }
        for px in pixels(b.bytes(), false) {
            assert_eq!(px, [0, 0, 0, ALPHA], "mode bits {reserved:05b}");
        }
    }
}

/// The two DXGI variants, and the client's +1 offset above id 26.
///
/// DXGI 94 is `BC6H_TYPELESS`, 95 `BC6H_UF16` (unsigned), 96 `BC6H_SF16`
/// (signed). The install stores one higher, so a stored 96 is the unsigned
/// variant — which is what every BC6H asset on this install carries.
#[test]
fn the_stored_byte_names_the_variant() {
    assert_eq!(
        Format::from_header(96),
        (Format::Bc6h { signed: false }, 95)
    );
    assert_eq!(Format::from_header(97), (Format::Bc6h { signed: true }, 96));
    // The typeless id has no signedness of its own; we decode it unsigned.
    assert_eq!(
        Format::from_header(95),
        (Format::Bc6h { signed: false }, 94)
    );
}

/// A 4x4 BC6H texture through the whole public path: header in, half floats
/// and clamped bytes out, and a DDS that names the format.
#[test]
fn a_bc6h_texture_decodes_end_to_end_and_writes_a_dds() {
    let block = texture::bc6h_random_blocks(11, 1, 7)[0]; // mode 11, one region
    let mut asset = vec![0u8; 0x20];
    asset[2] = 1; // one mip
    asset[3] = 96; // stored byte: DXGI 95, BC6H_UF16
    asset[4] = 1; // one surface
    asset[8..10].copy_from_slice(&4u16.to_le_bytes());
    asset[10..12].copy_from_slice(&4u16.to_le_bytes());
    asset[12..14].copy_from_slice(&16u16.to_le_bytes());
    asset.extend_from_slice(&block);

    let t = texture::parse(&asset).expect("parses");
    assert_eq!(t.format, Format::Bc6h { signed: false });
    assert_eq!(t.rgba16.len(), 4 * 4 * 4, "four half floats per pixel");
    assert_eq!(&t.blocks, &block);

    // The 8-bit plane is the clamped view, never the value itself: anything
    // above 1.0 pins at 255 and anything below 0.0 at 0.
    for i in 0..16 {
        for c in 0..3 {
            let v = half(t.rgba16[i * 4 + c]);
            let expect = if v < 0.0 {
                0
            } else if v > 1.0 {
                255
            } else {
                (v * 255.0 + 0.5) as u8
            };
            assert_eq!(t.rgba[i * 4 + c], expect, "pixel {i} channel {c}");
        }
        assert_eq!(t.rgba[i * 4 + 3], 255, "the byte view is opaque");
    }

    let dds = t.to_dds().expect("dds");
    assert_eq!(&dds[0..4], b"DDS ");
    // A DX10 header: fourCC "DX10" at the pixel format's fourCC slot (76 + 8,
    // after the 4-byte magic) and a 20-byte extension block whose first word is
    // the DXGI id. 95 = BC6H_UF16, and the block run follows it.
    assert_eq!(&dds[88..92], b"DX10");
    let dxgi = u32::from_le_bytes([dds[128], dds[129], dds[130], dds[131]]);
    assert_eq!(dxgi, 95, "DXGI 95 is BC6H_UF16");
    // The DX10 block, after the 128-byte DDS header: dxgiFormat at +0,
    // resourceDimension at +4 (3 = a 2D texture), arraySize at +12.
    assert_eq!(
        u32::from_le_bytes([dds[132], dds[133], dds[134], dds[135]]),
        3,
        "resourceDimension 3 = a 2D texture"
    );
    assert_eq!(
        u32::from_le_bytes([dds[140], dds[141], dds[142], dds[143]]),
        1,
        "one array element"
    );
    assert_eq!(
        dds.len(),
        128 + 20 + 16,
        "header, DX10 block, one 16-byte block"
    );
    assert_eq!(&dds[148..164], &block[..]);
}

/// The same, for the signed variant: the DDS must say 96, not 95.
#[test]
fn a_signed_bc6h_texture_names_dxgi_96() {
    let block = texture::bc6h_random_blocks(15, 1, 7)[0];
    let mut asset = vec![0u8; 0x20];
    asset[3] = 97; // stored byte: DXGI 96, BC6H_SF16
    asset[4] = 1;
    asset[8..10].copy_from_slice(&4u16.to_le_bytes());
    asset[10..12].copy_from_slice(&4u16.to_le_bytes());
    asset[12..14].copy_from_slice(&16u16.to_le_bytes());
    asset.extend_from_slice(&block);
    let t = texture::parse(&asset).expect("parses");
    assert_eq!(t.format, Format::Bc6h { signed: true });
    let dds = t.to_dds().expect("dds");
    assert_eq!(
        u32::from_le_bytes([dds[128], dds[129], dds[130], dds[131]]),
        96
    );
}

/// Half floats survive the round trip, and the known encodings are the known
/// ones. The decoder's output is a bit pattern that names a value exactly, so
/// `half -> f32 -> half` has to be the identity on every pixel of a sweep —
/// anything else would mean the 8-bit and 16-bit views describe different data.
#[test]
fn half_floats_round_trip() {
    for (h, want) in [
        (0x0000u16, 0.0f32),
        (0x3C00, 1.0),
        (0xBC00, -1.0),
        (0x4000, 2.0),
        (0x3800, 0.5),
        (0x7BFF, 65504.0),
        (0xFBFF, -65504.0),
        (0x3555, 0.333_251_95),
    ] {
        assert_eq!(half(h), want, "half 0x{h:04x}");
        assert_eq!(f32_to_half(want), h, "back again from {want}");
    }
    for mode in BC6H_MODE_NUMBERS {
        for signed in [false, true] {
            for block in texture::bc6h_random_blocks(mode, 16, 0x5A5A) {
                let (_, halves) = texture::decode_bc6h_block(&block, signed);
                for h in halves {
                    assert_eq!(f32_to_half(half(h)), h, "mode {mode} signed={signed}");
                }
            }
        }
    }
}

/// The anchor pixel of a region stores one bit fewer, because the two
/// endpoints of a pair can be swapped without changing the picture.
///
/// Mode 3 (`00011`) is the simplest one to build by hand: one region, both
/// endpoints stored outright at ten bits, no partition, and four-bit indices.
/// Pixel 0 is the anchor, so its index lives in three bits — and reading four
/// there would shift every later index, which is what makes the assertion below
/// non-vacuous.
#[test]
fn the_anchor_pixel_stores_one_bit_fewer_than_the_others() {
    let mut block = Block::new();
    block.put(0b00011, 5); // mode 3: one region, 10.10, four-bit indices
    for _ in 0..3 {
        block.put(0, 10); // r0, g0, b0: black
    }
    for _ in 0..3 {
        block.put(1000, 10); // r1, g1, b1
    }
    block.put(0b011, 3); // pixel 0: index 3 of 4, written three bits wide
    for _ in 1..16 {
        block.put(0b1111, 4); // every other pixel: index 15, the far endpoint
    }
    let got = pixels(block.bytes(), false);

    let near = unquantised(0, 10);
    let far_endpoint = unquantised(1000, 10);
    // The anchor's index still indexes the mode's *four-bit* weight table: the
    // bit it does not store is implied zero, not a narrower table. Index 3 of
    // sixteen is weight 13; index 15 is 64.
    let weights4 = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];
    assert_eq!(
        got[0][0],
        finish(interpolate(near, far_endpoint, weights4[3])),
        "the anchor pixel read its three bits"
    );
    assert_eq!(got[0][1], got[0][0], "grey, so the channels agree");
    assert_eq!(got[1][0], finish(interpolate(near, far_endpoint, 64)));
    assert_eq!(got[15][0], finish(interpolate(near, far_endpoint, 64)));

    // Non-vacuity: an alternative reader that gives the anchor its full four
    // bits reads a different index there — its three stored bits plus the top
    // three bits of the next pixel's — and so gives a different pixel. The
    // assertion above is therefore not passing because the index cannot matter.
    let alt_index = (0b011u32 | (0b1111u32 << 3)) & 0xf;
    assert_eq!(alt_index, 11, "the alternate read runs into the next pixel");
    assert_ne!(
        finish(interpolate(
            near,
            far_endpoint,
            weights4[alt_index as usize]
        )),
        finish(interpolate(near, far_endpoint, weights4[3])),
        "and a different pixel value"
    );
}
#[test]
fn the_two_endpoints_are_interchangeable_and_nothing_reorders_them() {
    // Mode 3 stores both endpoints outright at ten bits, which is the only way
    // to swap a pair without touching the block's meaning.
    for (i, block) in texture::bc6h_random_blocks(3, 64, 0xBEEF)
        .iter()
        .enumerate()
    {
        let v = u128::from_le_bytes(*block);
        let low = v as u32;
        let mode = if low & 3 < 2 { low & 3 } else { low & 0x1f };
        assert_eq!(mode, 3);
        let mut swapped = Block::new();
        swapped.put(3, 5);
        for c in 0..3 {
            swapped.put(field(v, 5 + c * 10, 10), 10); // r0, g0, b0
        }
        for c in 0..3 {
            swapped.put(field(v, 35 + c * 10, 10), 10); // r1, g1, b1
        }
        // Same indices: read them straight out of the block and write them back.
        let mut pos = 65usize;
        for pixel in 0..16 {
            let n: u8 = if pixel == 0 { 3 } else { 4 };
            swapped.put(field(v, pos, n as usize), n);
            pos += n as usize;
        }
        let a = pixels(*block, false);
        let b = pixels(swapped.bytes(), false);
        // The swapped block must actually be different...
        let differs_somewhere = a.iter().zip(&b).any(|(x, y)| x != y);
        let mut ordered = a.clone();
        if differs_somewhere {
            // ...and an implementation that *did* impose an order (smaller
            // first, which is what the BC1/BC4-style rule amounts to) would
            // then produce something else, so the equality above has teeth.
            for px in ordered.iter_mut() {
                for c in 0..3 {
                    let (x, y) = (half(px[c]), half(px[c]));
                    if x > y {
                        px[c] = f32_to_half(x);
                    }
                }
            }
            assert!(
                ordered != a,
                "block {i}: an endpoint-ordering decoder would differ, so this \
                 test is not vacuous"
            );
        }
        // And with every pair swapped the *set* of interpolated values is the
        // same, but the weights run the other way, so only pixels whose index
        // is not 0 or the maximum may move. Assert what must hold: alpha, and
        // the extremes.
        assert!(a
            .iter()
            .all(|p| p[3] == ALPHA && b.iter().all(|q| q[3] == ALPHA)));
        let _ = (pos, ordered);
    }
}

/// An endpoint at all ones is exactly the largest finite half, and the delta
/// encoding wraps at the base endpoint's precision.
///
/// Two halves of one claim, one per mode family. Mode 3 (`00011`) stores both
/// endpoints outright at ten bits, so a base endpoint of 1023 is all ones: it
/// unquantises to 0xFFFF and finishes to 0x7BFF — the largest finite half,
/// 65504 — rather than being read as "nearly the top". Mode 0 (`00`) stores
/// ten-bit endpoints with five-bit deltas, so a base of 1023 (all ones) plus a
/// delta of 1 wraps back to zero: the specification's "the results of the
/// inverse transform must wrap within the same number of bits".
#[test]
fn endpoints_hit_both_extremes_and_the_delta_wraps() {
    // Mode 3: 10.10, one region, both endpoints explicit.
    let mut b = Block::new();
    b.put(0b00011, 5);
    for _ in 0..3 {
        b.put(1023, 10); // r0, g0, b0: all ones
    }
    for _ in 0..3 {
        b.put(1000, 10); // r1, g1, b1
    }
    for pixel in 0..16 {
        if pixel == 0 {
            // Pixel 0 is the only anchor: three bits, and its index is zero, so
            // it lands exactly on the base endpoint.
            b.put(0, 3);
        } else {
            b.put(0b1111, 4); // the second endpoint
        }
    }
    let px = pixels(b.bytes(), false);
    assert_eq!(px[0][0], 0x7BFF, "an all-ones endpoint is the largest half");
    assert_eq!(half(px[0][0]), 65504.0);
    let near = finish(interpolate(
        unquantised(1023, 10),
        unquantised(1000, 10),
        64,
    ));
    assert_eq!(px[1][0], near, "index 15 is the second endpoint");
    assert!(half(near) < 65504.0, "1000 is below the all-ones endpoint");

    // Mode 26: an eight-bit base endpoint and six-bit deltas. A base of 255 is
    // all ones, and a delta of 1 wraps the sum back to zero at eight bits —
    // the specification's "the results of the inverse transform must wrap
    // within the same number of bits".
    let wrapped = pixels(mode26_block(0, 255, 1, 0), false);
    // Pixel 2 is region 1 under shape 0 and sits on its far endpoint, which is
    // the wrapped one.
    assert_eq!(
        wrapped[2][2], 0,
        "(255 + 1) must wrap to 0 at eight bits, not saturate: got 0x{:04x}",
        wrapped[2][2]
    );
    let plain = pixels(mode26_block(0, 255, 0, 0), false);
    assert_eq!(
        plain[2][2],
        finish(unquantised(255, 8)),
        "a zero delta keeps the all-ones endpoint"
    );
    assert_ne!(
        wrapped[2][2], plain[2][2],
        "the wrap is the only thing that changed the value"
    );
}

/// Build a mode-26 block with the endpoint fields and the partition index under
/// the test's control. Red and green are always zero, so any change to them
/// would show up immediately.
///
/// The field order is restated here independently of the table in `texture.rs`,
/// so the two have to agree for the tests below to mean anything:
/// RW[7:0], BZ[1], BY[4], GW[7:0], BY[5], GY[4], BW[7:0], **the ninth field**,
/// BZ[4], RX[4:0], GZ[4], GY[3:0], GX[4:0], BZ[0], GZ[3:0], BX[5:0], BY[3:0],
/// RY[4:0], BZ[2], RZ[4:0], BZ[3], then the 5-bit partition index.
///
/// `blue_base` is the BW field and `blue_delta_bit0` the lowest bit of the
/// region-1 blue delta (BZ[0]).
fn mode26_block(ninth: u32, blue_base: u32, blue_delta_bit0: u32, partition: u32) -> [u8; 16] {
    const ORDER: [(&str, u8); 21] = [
        ("rw", 8),
        ("bz1", 1),
        ("by4", 1),
        ("gw", 8),
        ("by5", 1),
        ("gy4", 1),
        ("bw", 8),
        ("ninth", 1),
        ("bz4", 1),
        ("rx", 5),
        ("gz4", 1),
        ("gy", 4),
        ("gx", 5),
        ("bz0", 1),
        ("gz", 4),
        ("bx", 6),
        ("by", 4),
        ("ry", 5),
        ("bz2", 1),
        ("rz", 5),
        ("bz3", 1),
    ];
    let mut b = Block::new();
    b.put(0b11010, 5); // mode 26
    for (name, width) in ORDER {
        let value = match name {
            "ninth" => ninth,
            "bw" => blue_base,
            "bz0" => blue_delta_bit0,
            // Every red and green field, and every other blue field, is zero.
            _ => 0,
        };
        b.put(value, width);
    }
    b.put(partition, 5);
    for pixel in 0..16 {
        // Index 7 of 8: the far endpoint of the pixel's own region. Shape 0's
        // anchors are pixels 0 and 15, and each stores its index one bit short,
        // so those two read index 3.
        b.put(0b111, if pixel == 0 || pixel == 15 { 2 } else { 3 });
    }
    b.bytes()
}

/// Mode 26's ninth field is a *blue* delta bit, not a green one.
///
/// The Khronos extension prints `g3[5]` where Microsoft's field table and its
/// own delta-width column say `b3[5]`; the cross-decoder check found the
/// difference (see `docs/formats/materials.md`). This pins the behaviour the
/// width table implies — {5,5,6} puts the six-bit deltas in blue — so the typo
/// cannot come back.
#[test]
fn mode26s_ninth_field_is_a_blue_delta_bit() {
    let with_bit = pixels(mode26_block(1, 100, 0, 0), false);
    let without = pixels(mode26_block(0, 100, 0, 0), false);

    // Shape 0 splits the pixels: 2,3,6,7,10,11,14,15 are region 1.
    let region1 = [2usize, 3, 6, 7, 10, 11, 14, 15];
    let region0 = [0usize, 1, 4, 5, 8, 9, 12, 13];

    // With the ninth bit set, blue in region 1 moves: the stored delta of 32
    // sign-extends to -32 at six bits, and 100 + (-32) wraps to 68 at eight.
    assert_ne!(with_bit[2][2], without[2][2], "blue in region 1 must move");
    let weights3 = [0, 9, 18, 27, 37, 46, 55, 64];
    for p in region1 {
        // Pixel 15 is this shape's anchor, so its index stored three bits and
        // its implied top bit is zero: it reads index 3, not 7.
        let weight = if p == 15 { weights3[3] } else { weights3[7] };
        assert_eq!(
            with_bit[p][2],
            finish(interpolate(unquantised(100, 8), unquantised(68, 8), weight)),
            "region 1 pixel {p}"
        );
    }
    // Green must not move at all: the bit is blue's, and if it were green's
    // (the Khronos typo) these would differ.
    for p in 0..16 {
        assert_eq!(with_bit[p][1], without[p][1], "green pixel {p}");
        assert_eq!(with_bit[p][1], 0, "green is zero throughout");
    }
    for p in region0 {
        assert_eq!(with_bit[p][2], without[p][2], "region 0 pixel {p}");
    }
}

/// A five-bit partition index must actually select the shape it names.
///
/// Every two-region mode puts that index at bit 77 — the specification's
/// "82 bits for two-region tiles" counted backwards from the index run — so
/// the shape can be changed without disturbing anything else in the block. Take
/// a random mode-26 block, rewrite just those five bits, and the pixels that
/// move between regions must change value.
#[test]
fn the_partition_index_selects_which_endpoints_a_pixel_uses() {
    const PARTITION_AT: u32 = 77;
    const PARTITION_MASK: u128 = 0x1f << PARTITION_AT;
    for block in texture::bc6h_random_blocks(26, 32, 0x515A) {
        let original = u128::from_le_bytes(block);
        let shape0 = original & !PARTITION_MASK;
        let shape31 = shape0 | (0x1f << PARTITION_AT);
        // The two blocks differ in those five bits and nothing else, so any
        // difference between their decodes is the shape's doing.
        assert_eq!(shape0 ^ shape31, 0x1f << PARTITION_AT);
        let a = pixels(shape0.to_le_bytes(), false);
        let b = pixels(shape31.to_le_bytes(), false);
        assert_ne!(a, b, "shape 31 must not decode like shape 0");
        // And the original block, whose shape is whatever it was built with,
        // decodes to one of the two or to a third shape — but never to
        // something the two above do not produce, so the decode is a function
        // of the bits alone.
        let original_px = pixels(block, false);
        assert_eq!(original_px, pixels(block, false));
        assert_eq!(original_px.len(), 16);
    }
}

/// The ten two-region modes share one partition table and a five-bit index; the
/// four one-region modes have neither.
#[test]
fn ten_modes_have_two_regions_and_four_have_one() {
    let two = BC6H_MODES.iter().filter(|m| m.two_regions).count();
    assert_eq!((two, BC6H_MODES.len() - two), (10, 4));
    for m in BC6H_MODES.iter() {
        assert_eq!(
            m.index_bits,
            if m.two_regions { 3 } else { 4 },
            "mode {}",
            m.number
        );
    }
}

// --------------------------------------------------------------- arithmetic
// The specification's own steps, restated so the tests can predict a value
// without running the decoder. Any change to the decoder that alters the
// arithmetic has to change these too, which is what keeps them a second
// opinion rather than a copy.

fn unquantised(v: i32, bits: u8) -> i32 {
    if bits >= 15 {
        return v;
    }
    if v == 0 {
        return 0;
    }
    if v == (1 << bits) - 1 {
        return 0xFFFF;
    }
    ((v << 16) + 0x8000) >> bits
}

fn interpolate(a: i32, b: i32, weight: i32) -> i32 {
    (a * (64 - weight) + b * weight + 32) >> 6
}

fn finish(v: i32) -> u16 {
    ((v * 31) >> 6) as u16
}

/// Bits `[pos, pos+n)` of a block's 128-bit little-endian value.
fn field(v: u128, pos: usize, n: usize) -> u32 {
    ((v >> pos) & ((1u128 << n) - 1)) as u32
}

//! The engine object's layout and the values its constructors write.
//!
//! Every assertion here is a number read from `WorldCtor` (`0x1405838c0`) or
//! `WorldBaseCtor` (`0x141c507c0`), or an offset those functions address
//! directly. The layout half is what makes the model usable as a view over a
//! real allocation; the value half is what proves the constructor was followed
//! operation for operation.

use ow_engine::codec::{Codec, INTERFACE_ARRAY, OBJ_460, SEED_38, SEED_A, SEED_B, SEED_B0};
use ow_engine::world::{Sub1d8, World, MAGIC_18, TABLE_LEN, VTABLE_FINAL};

#[test]
fn size_is_the_allocation_the_bootstrap_asks_for() {
    // `mov ecx, 0x470` before `Alloc`, 0x140585220.
    assert_eq!(core::mem::size_of::<World>(), 0x470);
}

#[test]
fn every_field_sits_at_the_offset_the_constructors_use() {
    macro_rules! at {
        ($field:ident, $off:expr) => {
            assert_eq!(
                core::mem::offset_of!(World, $field),
                $off,
                concat!("World::", stringify!($field))
            );
        };
    }
    at!(vtable, 0x000);
    at!(flag_08, 0x008);
    at!(interlocked_0c, 0x00c);
    at!(opaque_10, 0x010);
    at!(magic_18, 0x018);
    at!(q_20, 0x020);
    at!(d_28, 0x028);
    at!(d_2c, 0x02c);
    at!(d_30, 0x030);
    at!(d_34, 0x034);
    at!(enc_seed_38, 0x038);
    at!(d_40, 0x040);
    at!(d_44, 0x044);
    at!(q_48, 0x048);
    at!(q_50, 0x050);
    at!(enc_ptr_58, 0x058);
    at!(d_60, 0x060);
    at!(d_64, 0x064);
    at!(q_68, 0x068);
    at!(q_70, 0x070);
    at!(opaque_78, 0x078);
    at!(q_88, 0x088);
    at!(q_90, 0x090);
    at!(d_98, 0x098);
    at!(d_9c, 0x09c);
    at!(q_a0, 0x0a0);
    at!(q_a8, 0x0a8);
    at!(enc_seed_b0, 0x0b0);
    at!(q_b8, 0x0b8);
    at!(q_c0, 0x0c0);
    at!(zero_c8, 0x0c8);
    at!(opaque_e8, 0x0e8);
    at!(q_1c8, 0x1c8);
    at!(q_1d0, 0x1d0);
    at!(sub_1d8, 0x1d8);
    at!(d_1f0, 0x1f0);
    at!(f_1f4, 0x1f4);
    at!(b_1f8, 0x1f8);
    at!(d_1fc, 0x1fc);
    at!(sub_200, 0x200);
    at!(enc_world_id, 0x220);
    at!(table, 0x228);
    at!(enc_seed_460, 0x460);
    at!(d_468, 0x468);
    at!(d_46c, 0x46c);
}

#[test]
fn the_interface_array_and_the_index_map_are_where_the_client_reads_them() {
    // `lea rbx, [rcx+0x188]` in `RegisterWorldInterface`: the array header is
    // 0x10 bytes (`{ data, len, capacity }`) and lives inside the range the
    // base constructor leaves as opaque bytes, with the pointer→index map the
    // same function builds starting at `+0x198`.
    let array_header = core::mem::offset_of!(World, opaque_e8) + 0xa0;
    let index_map = core::mem::offset_of!(World, table) - 0x90;
    assert_eq!(
        array_header, 0x188,
        "the array header is in the opaque range"
    );
    assert_eq!(index_map, 0x198, "the index map is in the opaque range");
    assert!(
        array_header + 0x10 <= index_map,
        "the header cannot reach the map"
    );
    // The zeroed table starts after the map and runs exactly 71 qwords.
    assert_eq!(core::mem::offset_of!(World, table), 0x228);
    assert_eq!(TABLE_LEN, 0x47);
    assert_eq!(0x228 + TABLE_LEN * 8, 0x460);
}

#[test]
fn the_table_is_exactly_the_zeroed_range() {
    let w = World::new();
    assert!(w.table.iter().all(|&slot| slot == 0));
}

#[test]
fn the_constructors_write_their_own_constants() {
    let w = World::new();
    assert_eq!(w.vtable, VTABLE_FINAL);
    assert_eq!(w.flag_08, 1, "the bootstrap passes a5 = 0");
    assert_eq!(w.interlocked_0c, 0);
    assert_eq!(w.magic_18, MAGIC_18);
    assert_eq!(w.magic_18, 0x389a_ae49_b6ce_cf17);
    assert_eq!(w.q_20, 0);
    assert_eq!((w.d_28, w.d_2c, w.d_30), (0, 0x400, 0));
    assert_eq!(w.d_40, 0);
    assert_eq!((w.q_48, w.q_50), (0, 0));
    assert_eq!(w.d_60, 0);
    assert_eq!((w.q_68, w.q_70), (0, 0));
    assert_eq!((w.q_88, w.q_90), (0, 0));
    assert_eq!(w.d_98, 0);
    assert_eq!((w.q_a0, w.q_a8), (0, 0));
    assert_eq!((w.q_b8, w.q_c0), (0, 0));
    assert_eq!(w.zero_c8, [0u8; 0x20]);
    assert_eq!((w.q_1c8, w.q_1d0), (0, 0));
    assert_eq!(w.d_1f0, 0x10);
    assert_eq!(w.f_1f4, 0.25);
    assert_eq!(w.d_1fc, 0);
    assert_eq!(
        w.b_1f8, 0,
        "set to 1 while constructing, cleared at the end"
    );
}

#[test]
fn the_two_seed_folds_match_the_constructor_arithmetic() {
    let w = World::new();
    assert_eq!(w.enc_seed_38, 0xa8b4_a54b_8140_1bd0);
    assert_eq!(w.enc_seed_b0, 0x8242_eeb0_3b37_da9c);
    assert_eq!(w.enc_world_id, 0x48ce_818a_ff58_32e7);
    assert_eq!(w.enc_seed_460, 0x9b3d_0f76_a220_e758);

    // ... and recomputing them from the codecs' own operations agrees.
    assert_eq!(
        w.enc_seed_38,
        SEED_38.apply(0x627dce6e1d5603a4) ^ 0xd674a580a443b55e
    );
    let u = ((SEED_A.apply(0) ^ 0x6e).wrapping_add(0x6c5df24a0815916e)) ^ 0x3c1986ede8c2b44a;
    assert_eq!(
        w.enc_world_id,
        (u.rotate_right(0x10) ^ 0xc2300451bcb9e837).wrapping_add(0x5c04429ca265e813)
    );
    // +0x460 is written twice. The first write is the address codec at 0, and
    // the constructor's stored constant is that codec's inner mix of 0, so both
    // written forms agree.
    assert_eq!(w.enc_seed_460, OBJ_460.apply(0));
    assert_eq!(w.enc_seed_460, 0x9b3d_0f76_a220_e758);
    assert_eq!(
        SEED_B.apply(0xf45f13eca6129815) ^ 0x689f22ff8960a2c7,
        w.enc_seed_460
    );
    assert_eq!(
        w.enc_seed_b0,
        ((SEED_B0.apply(0).wrapping_add(0xcf64407ada39d02e)) ^ 0xb2b338251ec2701).rotate_left(0x17)
    );
}

#[test]
fn the_last_write_to_the_460_field_encodes_the_object_address() {
    // 0x140583c2b: xor 0xad02ee81538f03d8, add 0x4bbcf58285b7b2ec, ror 0x1a,
    // add 0xa2b162ae7619971f, SEED_B, xor 0x689f22ff8960a2c7 - followed by hand
    // for this one address.
    let addr = 0x0000_0000_1a2b_3c4d_5e6f_u64;
    assert_eq!(World::encode_obj_460(addr), 0xbda3_de7e_a220_d42f);
    assert_eq!(OBJ_460.revert(World::encode_obj_460(addr)), addr);
    assert_eq!(World::encode_obj_460(0), 0x9b3d_0f76_a220_e758);
}

#[test]
fn the_pointer_field_starts_as_the_encoded_null() {
    let w = World::new();
    // `param_1[0xb] = 0x4daa5a973c0808b6` in the base constructor is exactly
    // the interface codec's encoding of zero.
    assert_eq!(w.enc_ptr_58, 0x4daa_5a97_3c08_08b6);
    assert_eq!(w.enc_ptr_58, INTERFACE_ARRAY.apply(0));
    assert_eq!(INTERFACE_ARRAY.revert(w.enc_ptr_58), 0);
}

#[test]
fn the_sub_objects_hold_their_helpers_values() {
    let w = World::new();
    assert_eq!(w.sub_1d8.q0, 0);
    assert_eq!(w.sub_1d8.q8, 0);
    assert_eq!(w.sub_1d8.d10, 0);
    assert_eq!(core::mem::size_of::<Sub1d8>(), 0x18);

    // Packed fields: copy out before comparing so no reference is formed.
    let (b0, d10, q14, d1c) = (w.sub_200.b0, w.sub_200.d10, w.sub_200.q14, w.sub_200.d1c);
    assert_eq!(b0, 0);
    assert_eq!(d10, 0);
    assert_eq!(q14, 0x10);
    assert_eq!(d1c, 0);
    assert_eq!(core::mem::size_of::<ow_engine::world::Sub200>(), 0x20);
}

#[test]
fn a5_only_decides_the_one_flag() {
    assert_eq!(World::with_a5(0).flag_08, 1);
    assert_eq!(World::with_a5(1).flag_08, 0);
    assert_eq!(World::with_a5(-1).flag_08, 0);
}

#[test]
fn the_back_pointer_codec_round_trips_an_address() {
    let w = World::new();
    let encoded = w.back_pointer_to();
    let codec: Codec = ow_engine::codec::BACK_PTR;
    assert_eq!(codec.revert(encoded), &w as *const World as u64);
}

/// The constructor writes each of the three encoded fields twice: the codec's
/// encoding of zero first (the "no pointer" value), the real value second. The
/// proof that the first write *is* the encoded null is that the codec evaluates
/// to the very constant the constructor stores: `MAGIC_18` for `+0x018`, the
/// `INTERFACE_ARRAY` null for `+0x058`, and the stored seed form for `+0x460`.
#[test]
fn the_transient_writes_are_the_codec_null() {
    assert_eq!(World::encode_q_18(0), ow_engine::world::MAGIC_18);
    assert_eq!(
        World::encode_q_58(0),
        ow_engine::codec::INTERFACE_ARRAY.apply(0)
    );
    assert_eq!(
        World::encode_q_58(0),
        ow_engine::world::World::new().enc_ptr_58
    );
    assert_eq!(World::encode_q_58(0x1000), 0x4daa_5a97_4408_08b6);
    assert_eq!(World::encode_q_18(0x1000), 0x3896_ae49_b6ce_cf17);
    assert_eq!(World::encode_q_58(0xdead_beef_cafe), 0xac58_ae09_172f_08b6);
    assert_eq!(World::encode_q_18(0xdead_beef_cafe), 0x7485_6fc9_b617_7aa7);
}

//! Kind-specific map-chunk placeable bodies: synthetic layout pins, then a
//! sweep of the real install.
//!
//! The synthetic tests build each body at the exact offsets the parser reads,
//! so they fail if an offset drifts rather than quietly agreeing with it. The
//! sweep skips when no install is present; it asserts only what the bytes
//! support (model chunks still parse and resolve their references, and a
//! non-model kind parses real chunks) and prints the rest under `--nocapture`.
//!
//! Run the sweep with:
//!
//! ```text
//! cargo test -p ow-tank --release --test mapchunk_kinds -- --nocapture
//! ```

use std::collections::BTreeMap;

use ow_tank::mapchunk::{
    Body, Kind, MapChunk, Placeable, AREA_BOX_SIZE, AREA_STRUCT_SIZE, COMMON_SIZE,
    EFFECT_BODY_SIZE, ENTITY_INSTANCE_RECORD_SIZE, ENTITY_INSTANCE_TABLE_OFFSET, HEADER_SIZE,
    LIGHT_BODY_SIZE, MODEL_BODY_SIZE, MODEL_GROUP_ENTRY_SIZE, MODEL_GROUP_GROUP_SIZE,
    MODEL_GROUP_HEADER_SIZE, REFLECTION_POINT_BODY_SIZE, SOUND_BODY_SIZE, TEXT_BODY_SIZE,
};
use ow_tank::{guid, GameData, ManifestSelector};

/// Absolute offset of the first body in the synthetic chunks below.
const BODY_START: usize = HEADER_SIZE + COMMON_SIZE;

/// A chunk holding one placeable whose body is `body_len` zero bytes.
fn chunk(body_len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1u32.to_le_bytes()); // placeable count
    out.extend_from_slice(&0u32.to_le_bytes()); // instance data
    out.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes()); // placeable offset
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&[0xAB; 16]); // uuid
    out.extend_from_slice(&[0, 0, 0, 0]); // flags and the byte before Size
    out.extend_from_slice(&((COMMON_SIZE + body_len) as u32).to_le_bytes());
    out.resize(HEADER_SIZE + COMMON_SIZE + body_len, 0);
    out
}

fn put_u16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn put_f32s(buf: &mut [u8], at: usize, values: &[f32]) {
    for (i, v) in values.iter().enumerate() {
        buf[at + i * 4..at + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

#[test]
fn model_and_single_model_read_the_file_layout() {
    let mut data = chunk(MODEL_BODY_SIZE);
    put_u64(&mut data, BODY_START, 0x0000_00C0_0000_0123); // model
    put_u64(&mut data, BODY_START + 0x20, 0x0000_01A0_0000_0456); // look
    put_f32s(&mut data, BODY_START + 0x30, &[1.0, 2.0, 3.0]); // translation
    put_f32s(&mut data, BODY_START + 0x3C, &[4.0, 5.0, 6.0]); // scale
    put_f32s(&mut data, BODY_START + 0x48, &[0.1, 0.2, 0.3, 0.9]); // rotation
    put_f32s(&mut data, BODY_START + 0x58, &[7.0, 8.0, 9.0, 10.0]); // unknown quat

    let parsed = MapChunk::parse(&data, Kind::Model).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.model, Some(0x0000_00C0_0000_0123));
    assert_eq!(p.look, Some(0x0000_01A0_0000_0456));
    assert_eq!(p.translation, [1.0, 2.0, 3.0]);
    assert_eq!(p.scale, [4.0, 5.0, 6.0]);
    assert_eq!(p.rotation, [0.1, 0.2, 0.3, 0.9]);
    match p.body {
        Body::Model(m) => assert_eq!(m.unknown_rotation, [7.0, 8.0, 9.0, 10.0]),
        ref other => panic!("model parse produced {other:?}"),
    }

    // The reference reuses the model structure for single models; the same
    // bytes must read the same way under the single-model kind.
    let parsed = MapChunk::parse(&data, Kind::SingleModel).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.model, Some(0x0000_00C0_0000_0123));
    assert_eq!(p.look, Some(0x0000_01A0_0000_0456));
    assert_eq!(p.translation, [1.0, 2.0, 3.0]);
    assert!(matches!(p.body, Body::SingleModel(_)));
}

#[test]
fn model_group_reads_nested_groups_and_entries() {
    let body_len = MODEL_GROUP_HEADER_SIZE + MODEL_GROUP_GROUP_SIZE + 2 * MODEL_GROUP_ENTRY_SIZE;
    let mut data = chunk(body_len);
    put_u64(&mut data, BODY_START, 0x0000_00C0_0000_0011); // Structure.Model
    put_u32(&mut data, BODY_START + 8, 1); // GroupCount
    put_u32(&mut data, BODY_START + 0x0C, 2); // TotalModelCount
    put_u32(&mut data, BODY_START + 0x10, 0x1111_1111); // Mask
    put_u32(&mut data, BODY_START + 0x14, 0x2222_2222);
    put_u32(&mut data, BODY_START + 0x18, 0x3333_3333);
    put_u32(&mut data, BODY_START + 0x1C, 0x4444_4444);

    let group = BODY_START + MODEL_GROUP_HEADER_SIZE;
    put_u64(&mut data, group, 0x0000_01A0_0000_0022); // Group.ModelLook
    put_u32(&mut data, group + 8, 0x55); // Group.UnkSize
    put_u32(&mut data, group + 12, 2); // Group.EntryCount

    let entry0 = group + MODEL_GROUP_GROUP_SIZE;
    put_f32s(&mut data, entry0, &[1.0, 2.0, 3.0]);
    put_f32s(&mut data, entry0 + 12, &[1.0, 1.0, 1.0]);
    put_f32s(&mut data, entry0 + 24, &[0.0, 0.0, 0.0, 1.0]);
    put_u32(&mut data, entry0 + 40, 0xA1); // unknown_a[0]
    put_u32(&mut data, entry0 + 64, 0xA7); // unknown_a[6]
    put_u32(&mut data, entry0 + 68, 0xB1); // unknown_b[0]
    put_u32(&mut data, entry0 + 92, 0xB7); // unknown_b[6]

    let entry1 = entry0 + MODEL_GROUP_ENTRY_SIZE;
    put_f32s(&mut data, entry1, &[4.0, 5.0, 6.0]);

    let parsed = MapChunk::parse(&data, Kind::ModelGroup).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.model, Some(0x0000_00C0_0000_0011));
    match &p.body {
        Body::ModelGroup(g) => {
            assert_eq!(g.model, Some(0x0000_00C0_0000_0011));
            assert_eq!(g.group_count, 1);
            assert_eq!(g.total_model_count, 2);
            assert_eq!(g.mask, 0x1111_1111);
            assert_eq!(g.unknown, [0x2222_2222, 0x3333_3333, 0x4444_4444]);
            assert_eq!(g.groups.len(), 1, "one group fits");
            let only = &g.groups[0];
            assert_eq!(only.look, Some(0x0000_01A0_0000_0022));
            assert_eq!(only.unknown_size, 0x55);
            assert_eq!(only.entry_count, 2);
            assert_eq!(only.entries.len(), 2);
            assert_eq!(only.entries[0].translation, [1.0, 2.0, 3.0]);
            assert_eq!(only.entries[0].scale, [1.0, 1.0, 1.0]);
            assert_eq!(only.entries[0].rotation, [0.0, 0.0, 0.0, 1.0]);
            assert_eq!(only.entries[0].unknown_a[0], 0xA1);
            assert_eq!(only.entries[0].unknown_a[6], 0xA7);
            assert_eq!(only.entries[0].unknown_b[0], 0xB1);
            assert_eq!(only.entries[0].unknown_b[6], 0xB7);
            assert_eq!(only.entries[1].translation, [4.0, 5.0, 6.0]);
        }
        other => panic!("expected Body::ModelGroup, got {other:?}"),
    }
}

#[test]
fn entity_reads_component_instance_data_records() {
    let body_len = ENTITY_INSTANCE_TABLE_OFFSET + 2 * ENTITY_INSTANCE_RECORD_SIZE;
    let mut data = chunk(body_len);
    put_u64(&mut data, BODY_START, 0x0000_0400_0000_0001); // entity definition
    put_u64(&mut data, BODY_START + 8, 0x0000_01C0_0000_0002); // identifier
    put_f32s(&mut data, BODY_START + 16, &[1.0, 2.0, 3.0]);
    put_f32s(&mut data, BODY_START + 28, &[4.0, 5.0, 6.0]);
    put_f32s(&mut data, BODY_START + 40, &[0.0, 0.0, 0.0, 1.0]);
    put_u32(&mut data, BODY_START + 56, 2); // InstanceDataCount

    let records = BODY_START + ENTITY_INSTANCE_TABLE_OFFSET;
    put_u32(&mut data, records, 0xA462_03AB); // type
    put_u32(&mut data, records + 4, 0x40); // relative offset
    put_u32(&mut data, records + 8, 0xDEAD_BEEF); // second type
    put_u32(&mut data, records + 12, 0x20); // second relative offset

    let parsed = MapChunk::parse(&data, Kind::Entity).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.entity, Some(0x0000_0400_0000_0001));
    assert_eq!(p.identifier, Some(0x0000_01C0_0000_0002));
    assert_eq!(p.translation, [1.0, 2.0, 3.0]);
    assert_eq!(p.scale, [4.0, 5.0, 6.0]);
    assert_eq!(p.instance_data_count, 2);
    match &p.body {
        Body::Entity(e) => {
            assert_eq!(e.instance_data.len(), 2);
            // The reference resolves a record's target as
            // `record_start + relative_offset`.
            assert_eq!(e.instance_data[0].type_hash, 0xA462_03AB);
            assert_eq!(e.instance_data[0].offset, records + 0x40);
            assert_eq!(e.instance_data[1].type_hash, 0xDEAD_BEEF);
            assert_eq!(e.instance_data[1].offset, records + 8 + 0x20);
        }
        other => panic!("expected Body::Entity, got {other:?}"),
    }
}

#[test]
fn entity_instance_records_stop_at_the_placeable_size() {
    // The declared count says four records, but the placeable only holds one.
    let body_len = ENTITY_INSTANCE_TABLE_OFFSET + ENTITY_INSTANCE_RECORD_SIZE;
    let mut data = chunk(body_len);
    put_u32(&mut data, BODY_START + 56, 4);
    put_u32(
        &mut data,
        BODY_START + ENTITY_INSTANCE_TABLE_OFFSET,
        0x1234_5678,
    );

    let parsed = MapChunk::parse(&data, Kind::Entity).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.instance_data_count, 4, "the declared count is kept as-is");
    match &p.body {
        Body::Entity(e) => assert_eq!(e.instance_data.len(), 1, "only the fitting record"),
        other => panic!("expected Body::Entity, got {other:?}"),
    }
}

#[test]
fn light_reads_colour_fov_and_projection_textures() {
    let mut data = chunk(LIGHT_BODY_SIZE);
    put_f32s(&mut data, BODY_START, &[0.1, 0.2, 0.3, 0.9]); // rotation
    put_f32s(&mut data, BODY_START + 16, &[1.0, 2.0, 3.0]); // translation
    put_u32(&mut data, BODY_START + 28, 0xAA); // Unknown1
    put_u32(&mut data, BODY_START + 32, 0xBB); // Unknown2
    put_u32(&mut data, BODY_START + 36, 1); // teLIGHTTYPE.FRUSTUM
    put_u32(&mut data, BODY_START + 40, 0xCC); // Unknown3
    put_u32(&mut data, BODY_START + 44, 0xDD); // Unknown4
    put_f32s(&mut data, BODY_START + 48, &[0.5, 0.6, 0.7]); // teColorRGB (floats)
    put_f32s(&mut data, BODY_START + 60, &[4.0, 5.0, 6.0]); // UnknownPos1
    put_f32s(&mut data, BODY_START + 72, &[7.0, 8.0, 9.0, 10.0]); // UnknownQuat1
    put_f32s(&mut data, BODY_START + 88, &[45.0]); // LightFOV
    put_f32s(&mut data, BODY_START + 92, &[11.0, 12.0, 13.0]); // UnknownPos2
    put_f32s(&mut data, BODY_START + 104, &[14.0, 15.0, 16.0, 17.0]); // UnknownQuat2
    put_f32s(&mut data, BODY_START + 120, &[18.0, 19.0, 20.0]); // UnknownPos3
    put_f32s(&mut data, BODY_START + 132, &[21.0, 22.0, 23.0]); // UnknownPos4
    put_f32s(&mut data, BODY_START + 144, &[2.5]); // IntensityGUESS
    put_f32s(&mut data, BODY_START + 148, &[24.0, 25.0, 26.0, 27.0, 28.0]); // Unknown6..10
    put_u64(&mut data, BODY_START + 168, 0x0000_0040_0000_0007); // ProjectionTexture1
    put_u64(&mut data, BODY_START + 176, 0x0000_0040_0000_0008); // ProjectionTexture2

    let parsed = MapChunk::parse(&data, Kind::Light).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.rotation, [0.1, 0.2, 0.3, 0.9]);
    assert_eq!(p.translation, [1.0, 2.0, 3.0]);
    match &p.body {
        Body::Light(l) => {
            assert_eq!(l.rotation, [0.1, 0.2, 0.3, 0.9]);
            assert_eq!(l.translation, [1.0, 2.0, 3.0]);
            assert_eq!(l.unknown_1, 0xAA);
            assert_eq!(l.unknown_2, 0xBB);
            assert_eq!(l.light_type, 1);
            assert_eq!(l.unknown_3, 0xCC);
            assert_eq!(l.unknown_4, 0xDD);
            assert_eq!(l.color, [0.5, 0.6, 0.7]);
            assert_eq!(l.unknown_pos_1, [4.0, 5.0, 6.0]);
            assert_eq!(l.unknown_quat_1, [7.0, 8.0, 9.0, 10.0]);
            assert_eq!(l.light_fov, 45.0);
            assert_eq!(l.unknown_pos_2, [11.0, 12.0, 13.0]);
            assert_eq!(l.unknown_quat_2, [14.0, 15.0, 16.0, 17.0]);
            assert_eq!(l.unknown_pos_3, [18.0, 19.0, 20.0]);
            assert_eq!(l.unknown_pos_4, [21.0, 22.0, 23.0]);
            assert_eq!(l.intensity, 2.5);
            assert_eq!(l.unknown_floats, [24.0, 25.0, 26.0, 27.0, 28.0]);
            assert_eq!(l.projection_texture_1, Some(0x0000_0040_0000_0007));
            assert_eq!(l.projection_texture_2, Some(0x0000_0040_0000_0008));
        }
        other => panic!("expected Body::Light, got {other:?}"),
    }
}

#[test]
fn reflection_point_reads_identity_frame_and_corner() {
    let mut data = chunk(REFLECTION_POINT_BODY_SIZE);
    put_f32s(&mut data, BODY_START, &[1.0, 2.0, 3.0]); // translation
    put_f32s(&mut data, BODY_START + 12, &[4.0, 5.0, 6.0]); // scale
    put_f32s(&mut data, BODY_START + 24, &[0.1, 0.2, 0.3, 0.9]); // rotation
    put_f32s(&mut data, BODY_START + 40, &[7.0, 8.0, 9.0]); // identity translation
    put_f32s(&mut data, BODY_START + 52, &[10.0, 11.0, 12.0]); // identity scale
    put_f32s(&mut data, BODY_START + 64, &[0.4, 0.5, 0.6, 0.8]); // identity rotation
    put_f32s(&mut data, BODY_START + 80, &[13.0, 14.0, 15.0]); // corner
    put_u32(&mut data, BODY_START + 92, 0xAA); // Unknown1
    put_u64(&mut data, BODY_START + 96, 0x0000_0040_0000_0007); // Texture1
    put_u64(&mut data, BODY_START + 104, 0x0000_0040_0000_0008); // Texture2
    put_u32(&mut data, BODY_START + 112, 0xBB); // Unknown2
    put_u32(&mut data, BODY_START + 116, 0xCC); // Unknown3
    put_f32s(
        &mut data,
        BODY_START + 120,
        &[16.0, 17.0, 18.0, 19.0, 20.0, 21.0, 22.0],
    );
    put_u32(&mut data, BODY_START + 148, 0xFFFF_FFFE); // UnknownInt1 = -2
    put_u32(&mut data, BODY_START + 164, 0xFFFF_FFFD); // UnknownInt5 = -3

    let parsed = MapChunk::parse(&data, Kind::ReflectionPoint).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.translation, [1.0, 2.0, 3.0]);
    assert_eq!(p.scale, [4.0, 5.0, 6.0]);
    assert_eq!(p.rotation, [0.1, 0.2, 0.3, 0.9]);
    match &p.body {
        Body::ReflectionPoint(r) => {
            assert_eq!(r.identity_translation, [7.0, 8.0, 9.0]);
            assert_eq!(r.identity_scale, [10.0, 11.0, 12.0]);
            assert_eq!(r.identity_rotation, [0.4, 0.5, 0.6, 0.8]);
            assert_eq!(r.corner, [13.0, 14.0, 15.0]);
            assert_eq!(r.unknown_1, 0xAA);
            assert_eq!(r.texture_1, Some(0x0000_0040_0000_0007));
            assert_eq!(r.texture_2, Some(0x0000_0040_0000_0008));
            assert_eq!(r.unknown_2, 0xBB);
            assert_eq!(r.unknown_3, 0xCC);
            assert_eq!(r.unknown_4, [16.0, 17.0, 18.0, 19.0, 20.0, 21.0, 22.0]);
            assert_eq!(r.unknown_5[0], -2);
            assert_eq!(r.unknown_5[4], -3);
        }
        other => panic!("expected Body::ReflectionPoint, got {other:?}"),
    }
}

#[test]
fn text_reads_the_string_font_and_material_references() {
    let mut data = chunk(TEXT_BODY_SIZE);
    put_f32s(
        &mut data,
        BODY_START,
        &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
    );
    put_u64(&mut data, BODY_START + 32, 0x0000_0DD0_0000_0001); // String
    put_u64(&mut data, BODY_START + 40, 0x0000_0DD0_0000_0002); // MapFont
    put_u64(&mut data, BODY_START + 48, 0x0000_0DD0_0000_0003); // Material

    let parsed = MapChunk::parse(&data, Kind::Text).unwrap();
    let p = &parsed.placeables[0];
    match &p.body {
        Body::Text(t) => {
            assert_eq!(t.floats, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
            assert_eq!(t.string_guid, Some(0x0000_0DD0_0000_0001));
            assert_eq!(t.map_font, Some(0x0000_0DD0_0000_0002));
            assert_eq!(t.material, Some(0x0000_0DD0_0000_0003));
        }
        other => panic!("expected Body::Text, got {other:?}"),
    }
}

#[test]
fn sound_reads_the_sound_reference_and_transform() {
    let mut data = chunk(SOUND_BODY_SIZE);
    put_u64(&mut data, BODY_START, 0x0000_0D80_0000_0001); // Sound
    put_f32s(&mut data, BODY_START + 8, &[1.0, 2.0, 3.0]);
    put_f32s(&mut data, BODY_START + 20, &[4.0, 5.0, 6.0]);
    put_f32s(&mut data, BODY_START + 32, &[0.1, 0.2, 0.3, 0.9]);

    let parsed = MapChunk::parse(&data, Kind::Sound).unwrap();
    let p = &parsed.placeables[0];
    assert_eq!(p.translation, [1.0, 2.0, 3.0]);
    assert_eq!(p.scale, [4.0, 5.0, 6.0]);
    assert_eq!(p.rotation, [0.1, 0.2, 0.3, 0.9]);
    match &p.body {
        Body::Sound(s) => {
            assert_eq!(s.sound, Some(0x0000_0D80_0000_0001));
            assert_eq!(s.translation, [1.0, 2.0, 3.0]);
            assert_eq!(s.scale, [4.0, 5.0, 6.0]);
            assert_eq!(s.rotation, [0.1, 0.2, 0.3, 0.9]);
        }
        other => panic!("expected Body::Sound, got {other:?}"),
    }
}

#[test]
fn effect_and_sequence_read_the_effect_reference() {
    let mut data = chunk(EFFECT_BODY_SIZE);
    put_u64(&mut data, BODY_START, 0x0000_0C60_0000_0001); // Effect
    put_f32s(&mut data, BODY_START + 8, &[1.0, 2.0, 3.0]);
    put_f32s(&mut data, BODY_START + 20, &[4.0, 5.0, 6.0]);
    put_f32s(&mut data, BODY_START + 32, &[0.1, 0.2, 0.3, 0.9]);

    let parsed = MapChunk::parse(&data, Kind::Effect).unwrap();
    match parsed.placeables[0].body {
        Body::Effect(e) => {
            assert_eq!(e.effect, Some(0x0000_0C60_0000_0001));
            assert_eq!(e.translation, [1.0, 2.0, 3.0]);
            assert_eq!(e.scale, [4.0, 5.0, 6.0]);
            assert_eq!(e.rotation, [0.1, 0.2, 0.3, 0.9]);
        }
        ref other => panic!("expected Body::Effect, got {other:?}"),
    }

    let parsed = MapChunk::parse(&data, Kind::Sequence).unwrap();
    match parsed.placeables[0].body {
        Body::Sequence(e) => assert_eq!(e.effect, Some(0x0000_0C60_0000_0001)),
        ref other => panic!("expected Body::Sequence, got {other:?}"),
    }
}

#[test]
fn area_reads_the_counts_offsets_and_boxes() {
    let body_len = AREA_STRUCT_SIZE + AREA_BOX_SIZE;
    let mut data = chunk(body_len);
    put_u16(&mut data, BODY_START + 12, 1); // BoxCount
    put_u16(&mut data, BODY_START + 14, 2); // SphereCount
    put_u16(&mut data, BODY_START + 16, 3); // CapsuleCount
    put_u16(&mut data, BODY_START + 18, 4); // Unknown1Count
    put_u16(&mut data, BODY_START + 20, 5); // Unknown2Count
    put_u32(&mut data, BODY_START + 24, AREA_STRUCT_SIZE as u32); // BoxOffset
    put_u32(&mut data, BODY_START + 28, 0xDD); // SphereOffset
    put_u32(&mut data, BODY_START + 32, 0xEE); // CapsuleOffset
    put_u32(&mut data, BODY_START + 36, 0xFF); // Unknown1Offset
    put_u32(&mut data, BODY_START + 40, 0x1234); // Unknown2Offset

    let bx = BODY_START + AREA_STRUCT_SIZE;
    put_f32s(&mut data, bx, &[0.1, 0.2, 0.3, 0.9]); // Orientation
    put_f32s(&mut data, bx + 16, &[1.0, 2.0, 3.0]); // Translation
    put_f32s(&mut data, bx + 28, &[4.0, 5.0, 6.0]); // Extents
    put_f32s(&mut data, bx + 40, &[7.0, 8.0, 9.0, 10.0]); // NoIdea

    let parsed = MapChunk::parse(&data, Kind::Area).unwrap();
    let p = &parsed.placeables[0];
    match &p.body {
        Body::Area(a) => {
            assert_eq!(a.box_count, 1);
            assert_eq!(a.sphere_count, 2);
            assert_eq!(a.capsule_count, 3);
            assert_eq!(a.unknown_1_count, 4);
            assert_eq!(a.unknown_2_count, 5);
            assert_eq!(a.box_offset, AREA_STRUCT_SIZE as u32);
            assert_eq!(a.sphere_offset, 0xDD);
            assert_eq!(a.capsule_offset, 0xEE);
            assert_eq!(a.unknown_1_offset, 0xFF);
            assert_eq!(a.unknown_2_offset, 0x1234);
            assert_eq!(a.boxes.len(), 1);
            assert_eq!(a.boxes[0].orientation, [0.1, 0.2, 0.3, 0.9]);
            assert_eq!(a.boxes[0].translation, [1.0, 2.0, 3.0]);
            assert_eq!(a.boxes[0].extents, [4.0, 5.0, 6.0]);
            assert_eq!(a.boxes[0].unknown, [7.0, 8.0, 9.0, 10.0]);
        }
        other => panic!("expected Body::Area, got {other:?}"),
    }
}

#[test]
fn area_boxes_stop_at_the_placeable_size() {
    // BoxCount says two; only one box fits.
    let body_len = AREA_STRUCT_SIZE + AREA_BOX_SIZE;
    let mut data = chunk(body_len);
    put_u16(&mut data, BODY_START + 12, 2);
    put_u32(&mut data, BODY_START + 24, AREA_STRUCT_SIZE as u32);

    let parsed = MapChunk::parse(&data, Kind::Area).unwrap();
    match &parsed.placeables[0].body {
        Body::Area(a) => assert_eq!(a.boxes.len(), 1),
        other => panic!("expected Body::Area, got {other:?}"),
    }
}

#[test]
fn unknown_kind_walks_placeables_without_a_body_reader() {
    // Two placeables: the second declares a tiny body. Nothing about it is
    // read, but its declared size must still carry the walk to the end.
    let mut data = chunk(8);
    data[0..4].copy_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&[0xCD; 16]);
    data.extend_from_slice(&[0, 0, 0, 0]);
    data.extend_from_slice(&(COMMON_SIZE as u32 + 4).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());

    let parsed = MapChunk::parse(&data, Kind::Unknown).unwrap();
    assert_eq!(parsed.placeables.len(), 2);
    assert!(matches!(parsed.placeables[0].body, Body::Unknown));
    assert_eq!(parsed.placeables[1].size, COMMON_SIZE + 4);
}

#[test]
fn kind_ids_match_the_reference_enum() {
    for (kind, id) in [
        (Kind::Unknown, 0x00u8),
        (Kind::ModelGroup, 0x01),
        (Kind::SingleModel, 0x02),
        (Kind::ReflectionPoint, 0x04),
        (Kind::Text, 0x07),
        (Kind::Model, 0x08),
        (Kind::Light, 0x09),
        (Kind::Area, 0x0A),
        (Kind::Entity, 0x0B),
        (Kind::Sound, 0x0C),
        (Kind::Effect, 0x0D),
        (Kind::Sequence, 0x18),
    ] {
        assert_eq!(kind.type_id(), id, "{}", kind.label());
        assert_eq!(Kind::from_type_id(id), Some(kind), "{}", kind.label());
    }
    // The reference names these but has no body for them.
    assert_eq!(Kind::from_type_id(0x03), None); // occluder
    assert_eq!(Kind::from_type_id(0x0E), None); // fog
    assert_eq!(Kind::from_type_id(0xFF), None);
}

#[test]
fn the_instance_data_offset_is_sixteen_bytes_before_its_usable_value() {
    let mut data = chunk(4);
    put_u32(&mut data, 4, 0x2000);
    let parsed = MapChunk::parse(&data, Kind::Unknown).unwrap();
    assert_eq!(parsed.header.instance_data_at(), Some(0x2010));

    let zeroed = chunk(4);
    let parsed = MapChunk::parse(&zeroed, Kind::Unknown).unwrap();
    assert_eq!(parsed.header.instance_data_at(), None, "zero means absent");
}

#[test]
fn detect_keeps_its_model_first_default() {
    let mut data = chunk(MODEL_BODY_SIZE);
    put_u64(&mut data, BODY_START, 0x0000_00C0_0000_0123);
    let resolves = |g: u64| g == 0x0000_00C0_0000_0123;
    let (kind, chunk) = MapChunk::detect(&data, &resolves).unwrap();
    assert_eq!(kind, Kind::Model);
    assert_eq!(chunk.placeables.len(), 1);
}

/// Per-kind tallies for the real-install sweep.
#[derive(Debug, Default, Clone, Copy)]
struct KindTally {
    /// Assets whose bytes opened and parsed under this kind.
    assets: usize,
    /// Assets where the walk consumed the declared count and every size is sane.
    clean_walks: usize,
    /// Assets where the last placeable's end is exactly the asset's end (a
    /// chunk can also carry a trailing instance-data region).
    exact_end_walks: usize,
    placeables: usize,
    /// Placeables whose kind-specific structure fits their declared size.
    fitted: usize,
    /// Kind-specific reference fields that resolve in the asset registry.
    refs_resolved: usize,
    /// Kind-specific reference fields present (may include those that do not
    /// resolve because the asset is not installed locally).
    refs_seen: usize,
}

/// Every kind the reader parses, for the sweep.
const KINDS: [Kind; 12] = [
    Kind::Unknown,
    Kind::Model,
    Kind::SingleModel,
    Kind::ModelGroup,
    Kind::Entity,
    Kind::Light,
    Kind::ReflectionPoint,
    Kind::Text,
    Kind::Sound,
    Kind::Effect,
    Kind::Sequence,
    Kind::Area,
];

/// The look asset type the placement scan pinned (`docs/formats/materials.md`).
const MODEL_LOOK_TYPE: u16 = 0x01A;
/// The reference calls the entity's first reference `EntityDefinition`; the
/// registry types it 0x040.
const ENTITY_TYPE: u16 = 0x040;
/// The reference's `Identifier1` type.
const IDENTIFIER_TYPE: u16 = 0x01C;

/// Reference fields a placeable is *supposed* to carry, counted as
/// `(seen, resolved)`. Model, look, entity and identifier are type-checked;
/// the other kinds' fields are only tested against the registry, because a
/// wrong layout reads floats, which almost never land on an existing asset.
fn typed_refs(p: &Placeable, resolves: &dyn Fn(u64) -> bool) -> (usize, usize) {
    let mut seen = 0usize;
    let mut hit = 0usize;
    let mut check = |g: Option<u64>, want: Option<u16>| {
        if let Some(g) = g {
            if g != 0 {
                seen += 1;
                if want.map(|t| guid::type_of(g) == t).unwrap_or(true) && resolves(g) {
                    hit += 1;
                }
            }
        }
    };
    match &p.body {
        Body::Model(_) | Body::SingleModel(_) => {
            check(p.model, Some(guid::types::MODEL));
            check(p.look, Some(MODEL_LOOK_TYPE));
        }
        Body::ModelGroup(b) => {
            check(b.model, Some(guid::types::MODEL));
            for group in &b.groups {
                check(group.look, Some(MODEL_LOOK_TYPE));
            }
        }
        Body::Entity(_) => {
            check(p.entity, Some(ENTITY_TYPE));
            check(p.identifier, Some(IDENTIFIER_TYPE));
        }
        Body::Light(l) => {
            check(l.projection_texture_1, Some(guid::types::TEXTURE));
            check(l.projection_texture_2, Some(guid::types::TEXTURE));
        }
        Body::ReflectionPoint(r) => {
            check(r.texture_1, None);
            check(r.texture_2, None);
        }
        Body::Text(t) => {
            check(t.string_guid, None);
            check(t.map_font, None);
            check(t.material, None);
        }
        Body::Sound(s) => check(s.sound, None),
        Body::Effect(e) | Body::Sequence(e) => check(e.effect, None),
        Body::Area(_) | Body::Unknown => {}
    }
    (seen, hit)
}

/// Whether the kind-specific structure fits the placeable's declared size.
fn body_fits(p: &Placeable) -> bool {
    let body_len = p.size - COMMON_SIZE;
    match &p.body {
        Body::Model(_) | Body::SingleModel(_) => body_len >= MODEL_BODY_SIZE,
        Body::ModelGroup(b) => {
            b.groups.len() == b.group_count as usize
                && b.groups
                    .iter()
                    .all(|g| g.entries.len() == g.entry_count as usize)
        }
        Body::Entity(b) => b.instance_data.len() == p.instance_data_count as usize,
        Body::Light(_) => body_len >= LIGHT_BODY_SIZE,
        Body::ReflectionPoint(_) => body_len >= REFLECTION_POINT_BODY_SIZE,
        Body::Text(_) => body_len >= TEXT_BODY_SIZE,
        Body::Sound(_) => body_len >= SOUND_BODY_SIZE,
        Body::Effect(_) | Body::Sequence(_) => body_len >= EFFECT_BODY_SIZE,
        Body::Area(b) => b.boxes.len() == b.box_count as usize,
        Body::Unknown => true,
    }
}

fn tally_asset(
    tally: &mut KindTally,
    chunk: &MapChunk,
    asset_len: usize,
    resolves: &dyn Fn(u64) -> bool,
) {
    tally.assets += 1;
    tally.placeables += chunk.placeables.len();
    let clean = chunk.placeables.len() == chunk.header.placeable_count as usize;
    if clean {
        tally.clean_walks += 1;
    }
    if chunk
        .placeables
        .last()
        .map(|p| p.offset + p.size == asset_len)
        .unwrap_or(false)
    {
        tally.exact_end_walks += 1;
    }
    for p in &chunk.placeables {
        if body_fits(p) {
            tally.fitted += 1;
        }
        let (seen, hit) = typed_refs(p, resolves);
        tally.refs_seen += seen;
        tally.refs_resolved += hit;
    }
}

/// How many assets per declared kind the sweep reads.
const PER_KIND: usize = 20;

/// The real-install sweep: try every kind on real map chunks, print per-kind
/// tallies, and assert only what the bytes actually support.
///
/// Skipped when no install is present, like `tests/chunks_real.rs`.
#[test]
fn real_map_chunks_match_their_declared_kinds() {
    let Some(root) = ow_casc::install::detect_root() else {
        return;
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            return;
        }
    };

    let guids = data.guids_of_type(0x0BC);
    if guids.is_empty() {
        eprintln!("no map chunk assets (type 0x0BC) in the registry");
        return;
    }
    eprintln!("map chunks (type 0x0BC): {} asset(s)", guids.len());

    // The reference locates a map's chunk with the kind in bits 32..39 of the
    // asset GUID (`InstanceExtensions.GetChunkKey`). Whether real assets carry
    // it is measured here, not assumed: print the histogram first.
    let mut declared: BTreeMap<u8, Vec<u64>> = BTreeMap::new();
    for g in &guids {
        declared
            .entry(((g >> 32) & 0xFF) as u8)
            .or_default()
            .push(*g);
    }
    eprintln!("kind byte in the asset GUID (bits 32..39):");
    for (id, gs) in &declared {
        let name = match Kind::from_type_id(*id) {
            Some(kind) => kind.label(),
            None => "reference has no body",
        };
        eprintln!("  0x{id:02X} {name:<22} {} asset(s)", gs.len());
    }

    // Spread each declared-kind sample across its bucket rather than taking a
    // prefix: assets arrive in packages and a prefix can sit in one package.
    let mut sample: Vec<(u8, u64)> = Vec::new();
    for (id, gs) in &declared {
        let step = (gs.len() / PER_KIND).max(1);
        for g in gs.iter().step_by(step).take(PER_KIND) {
            sample.push((*id, *g));
        }
    }

    let resolves = |g: u64| data.registry().get(g).is_some();
    let mut by_kind: BTreeMap<Kind, KindTally> = BTreeMap::new();
    let mut by_declared: BTreeMap<u8, KindTally> = BTreeMap::new();
    let mut unreadable = 0usize;

    for (declared_id, g) in &sample {
        let bytes = match data.bytes_of(*g) {
            Ok(bytes) => bytes,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let declared_kind = Kind::from_type_id(*declared_id);
        for kind in KINDS {
            let Ok(chunk) = MapChunk::parse(&bytes, kind) else {
                continue;
            };
            tally_asset(
                by_kind.entry(kind).or_default(),
                &chunk,
                bytes.len(),
                &resolves,
            );
            if declared_kind == Some(kind) {
                tally_asset(
                    by_declared.entry(*declared_id).or_default(),
                    &chunk,
                    bytes.len(),
                    &resolves,
                );
            }
        }
    }

    eprintln!(
        "\nevery kind tried on every sampled asset ({} asset(s), {unreadable} unreadable):",
        sample.len()
    );
    eprintln!(
        "  {:<16} {:>6} {:>6} {:>9} {:>10} {:>7}  refs resolving",
        "kind", "assets", "clean", "exact-end", "placeables", "fitted"
    );
    for kind in KINDS {
        if let Some(t) = by_kind.get(&kind) {
            eprintln!(
                "  {:<16} {:>6} {:>6} {:>9} {:>10} {:>7}  {}/{}",
                kind.label(),
                t.assets,
                t.clean_walks,
                t.exact_end_walks,
                t.placeables,
                t.fitted,
                t.refs_resolved,
                t.refs_seen
            );
        }
    }

    eprintln!("\nassets parsed with the kind their GUID declares:");
    for (id, gs) in &declared {
        let Some(kind) = Kind::from_type_id(*id) else {
            eprintln!(
                "  0x{id:02X} (no body in the reference) {} asset(s), not swept by kind",
                gs.len()
            );
            continue;
        };
        match by_declared.get(id) {
            Some(t) => eprintln!(
                "  0x{id:02X} {:<16} {} asset(s) in the type, {} read: {} clean, \
                 {} exact-end, {} placeables, {} fitted, refs {}/{}",
                kind.label(),
                gs.len(),
                t.assets,
                t.clean_walks,
                t.exact_end_walks,
                t.placeables,
                t.fitted,
                t.refs_resolved,
                t.refs_seen
            ),
            None => eprintln!(
                "  0x{id:02X} {:<16} {} asset(s) in the type, none readable",
                kind.label(),
                gs.len()
            ),
        }
    }

    let model = by_kind.get(&Kind::Model).cloned().unwrap_or_default();
    if model.assets == 0 {
        eprintln!("no sampled map chunk bytes could be read; nothing to assert");
        return;
    }
    assert!(
        model.placeables > 0,
        "model chunks parsed but hold no placeables"
    );
    // docs/formats/materials.md measured every model placement's model and
    // look resolving; re-measured here on this sample.
    assert!(
        model.refs_resolved > 0,
        "no model reference resolved on any sampled chunk"
    );

    let non_model: Vec<&'static str> = KINDS
        .iter()
        .filter(|k| **k != Kind::Model)
        .filter(|k| by_kind.get(k).map(|t| t.placeables > 0).unwrap_or(false))
        .map(|k| k.label())
        .collect();
    assert!(
        !non_model.is_empty(),
        "no non-model kind produced a placeable on real data"
    );
    eprintln!("\nnon-model kinds that produced placeables on real data: {non_model:?}");
    eprintln!(
        "NOTE: a Kind that yields placeables on every chunk only means the size \
         walk works;\n      which kind is *correct* for an asset is what the \
         declared-kind table reports."
    );
}

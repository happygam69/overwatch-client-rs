//! Type-`0B3` material data and the client's texture-name binding.
//!
//! proof-covers: 0x141b1a7b0 0x141f1ca80 0x141b52ca0
//!
//! The parser's layout is read from the client's material-data load callback
//! (`FUN_141b1a7b0`, job 28) and its texture-set consumer (`FUN_141f1ca80`):
//! the count is the byte at `+0x3C`, the array offset the i64 at `+0x10`, and
//! a record is 16 bytes (`GUID`, `u32` input name hash, four flag bytes).
//! The hash is zlib CRC32 (`FUN_141b52ca0`; string wrapper `FUN_141bb9810`).
//!
//! This test reads the operator's install in place. It is skipped when no
//! install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test material_real -- --nocapture
//! # the full 121,035-asset census (about 6 minutes in release):
//! cargo test --release -p ow-tank --test material_real -- --ignored --nocapture
//! ```

use ow_tank::keytable::Sources;
use ow_tank::material::{
    crc32, input_hash, material_data_of, texture_of_hash, texture_of_input,
    texture_of_input_in_data, texture_records, textures_of_material, MaterialTexture,
};
use ow_tank::{guid, GameData, ManifestSelector};

fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

fn records(data: &GameData, asset: u64) -> Vec<MaterialTexture> {
    let md = data
        .open_asset(asset)
        .unwrap_or_else(|e| panic!("0x{asset:016x} must open when the install is present: {e}"));
    texture_records(&md)
}

const MATERIAL: u64 = 0x0E00_0000_0000_518A;
const MATERIAL_DATA: u64 = 0x04D0_0000_0000_85E8;

/// The five records the audit found, in array order.
const PINNED: [(u64, u32, u32); 5] = [
    (0x0C00_0000_0001_1D5E, 0xC6D1_7199, 0),
    (0x0C00_0000_0000_E61D, 0xBCBE_747D, 0),
    (0x0C00_0000_0000_E349, 0xAD10_F602, 0),
    (0x0C00_0000_0000_E34A, 0x20AF_06CE, 0),
    (0x0C00_0000_0000_E34B, 0x1696_15AA, 0),
];

fn expected(records: &[(u64, u32, u32)]) -> Vec<MaterialTexture> {
    records
        .iter()
        .map(|&(texture, input, flags)| MaterialTexture {
            texture,
            input,
            flags,
        })
        .collect()
}

#[test]
fn the_pinned_material_has_the_five_read_records() {
    let Some(data) = open() else {
        return;
    };
    let md = material_data_of(&data, MATERIAL)
        .unwrap_or_else(|e| panic!("material 0x{MATERIAL:016x} must open: {e}"));
    let material = data.open_asset(MATERIAL).expect("the material asset opens");
    assert_eq!(
        u64::from_le_bytes(material[0x18..0x20].try_into().unwrap()),
        MATERIAL_DATA
    );
    assert_eq!(guid::type_of(MATERIAL_DATA), 0x0B3);
    assert_eq!(md.len(), 304, "material data length changed");
    let records = texture_records(&md);
    assert_eq!(records, expected(&PINNED));

    // The hash is zlib CRC32 of the input name and it picks the record.
    assert_eq!(crc32(b"basecolor"), 0xAD10_F602);
    assert_eq!(input_hash("basecolor"), 0xAD10_F602);
    assert_eq!(
        texture_of_input(&data, MATERIAL, "basecolor"),
        Some(0x0C00_0000_0000_E349)
    );
    assert_eq!(
        texture_of_hash(&data, MATERIAL, 0xAD10_F602),
        Some(0x0C00_0000_0000_E349)
    );
    assert_eq!(texture_of_input(&data, MATERIAL, "BaseColor"), None);
    assert_eq!(
        textures_of_material(&data, MATERIAL).unwrap(),
        PINNED.iter().map(|r| r.0).collect::<Vec<_>>()
    );
}

/// `0x04D000000000358E` packs a texture count of 6 with a pointer-array count
/// of 1: the u32 at `+0x3C` is `0x106`. Reading it as a count would ask for 262
/// records and run past EOF; the client reads the low byte.
#[test]
fn the_packed_count_word_is_not_a_texture_count() {
    let Some(data) = open() else {
        return;
    };
    let md = data
        .open_asset(0x04D0_0000_0000_358E)
        .expect("0x04d000000000358e must open when the install is present");
    assert_eq!(
        u32::from_le_bytes(md[0x3C..0x40].try_into().unwrap()),
        0x106
    );
    let records = texture_records(&md);
    assert_eq!(records.len(), 6);
    assert_eq!(
        records
            .iter()
            .map(|r| (r.texture, r.input))
            .collect::<Vec<_>>(),
        vec![
            (0x0C00_0000_0000_03D3, 0xCDE3_0B73),
            (0x0C00_0000_0000_5469, 0xA503_0545),
            (0x0C00_0000_0000_000C, 0xA26E_C15C),
            (0x0C00_800F_EA97_8115, 0x5118_206D),
            (0x0C00_0000_0000_5434, 0x4B0D_6469),
            (0x0C00_0000_0000_542F, 0x3B67_90E6),
        ]
    );
}

/// The record GUID is not always type `0x004`: 83 of 693,111 records are type
/// `0x0F1` texture aliases across 73 payloads. The consumer copies the GUID
/// without a type test, so both are valid. These four aliases pin the family.
#[test]
fn the_alias_records_are_type_0f1_and_are_copied() {
    let Some(data) = open() else {
        return;
    };
    let aliases: [(u64, &[(u64, u32, u32)]); 4] = [
        (
            0x04D0_0000_0000_80A4,
            &[
                (0x0C00_0000_0000_F936, 0xBCBE_747D, 0),
                (0x00F0_0000_0000_000E, 0xAD10_F602, 0),
                (0x0C00_0000_0000_F939, 0x5CD3_F052, 0),
                (0x0C00_0000_0001_1C8F, 0x5862_A480, 0),
                (0x00F0_0000_0000_000F, 0x20AF_06CE, 0),
                (0x00F0_0000_0000_0010, 0x1696_15AA, 0),
            ],
        ),
        (
            0x04D0_0000_0000_8615,
            &[
                (0x0C00_0000_0000_000B, 0xD738_CC75, 0x8000),
                (0x0C00_0000_0000_F93A, 0xB317_E4FD, 0),
                (0x00F0_0000_0000_000E, 0xAD10_F602, 0),
                (0x0C00_0000_0000_000A, 0x9714_B253, 0x8000),
                (0x00F0_0000_0000_000F, 0x20AF_06CE, 0),
                (0x00F0_0000_0000_0010, 0x1696_15AA, 0),
                (0x0C00_0000_0000_01EE, 0x0CDA_8A9E, 0),
            ],
        ),
        (
            0x04D0_0000_0001_DBE1,
            &[
                (0x00F0_0000_0000_00F8, 0xBCBE_747D, 0),
                (0x00F0_0000_0000_0119, 0xAD10_F602, 0),
                (0x0C00_0000_0000_000A, 0x5862_A480, 0),
                (0x00F0_0000_0000_00F7, 0x20AF_06CE, 0),
                (0x00F0_0000_0000_00F9, 0x1696_15AA, 0),
            ],
        ),
        (
            0x04D0_0000_0001_E9DE,
            &[
                (0x00F0_0000_0000_00F8, 0xBCBE_747D, 0),
                (0x00F0_0000_0000_0119, 0xAD10_F602, 0),
                (0x0C00_0000_0000_000A, 0x5862_A480, 0),
                (0x00F0_0000_0000_00F7, 0x20AF_06CE, 0),
                (0x00F0_0000_0000_00F9, 0x1696_15AA, 0),
            ],
        ),
    ];
    for (asset, expected_records) in aliases {
        let got = records(&data, asset);
        assert_eq!(got, expected(expected_records), "0x{asset:016x}");
        let alias_count = got
            .iter()
            .filter(|r| guid::type_of(r.texture) == 0x0F1)
            .count();
        assert!(alias_count > 0, "0x{asset:016x} has a 0x0F1 record");
    }
    // The alias GUID is what the hash lookup returns, not an error. This is
    // the byte-level lookup on the material-data payload; the payload has no
    // material-asset wrapper here.
    assert_eq!(
        texture_of_input(&data, MATERIAL, "basecolor"),
        Some(0x0C00_0000_0000_E349)
    );
    let alias_data = 0x04D0_0000_0001_DBE1;
    let md = data
        .open_asset(alias_data)
        .expect("the alias payload opens");
    assert_eq!(
        texture_of_input_in_data(&md, "basecolor"),
        Some(0x00F0_0000_0000_0119)
    );
}

/// The shader reflection data carries `0xAD10F602`. This is install evidence
/// that the material-record and shader-resource name hashes coincide; the
/// reflection loader shape is read, but the material→shader join is not.
#[test]
fn shader_reflection_data_carries_the_basecolor_hash() {
    let Some(data) = open() else {
        return;
    };
    let reflection = data
        .open_asset(0x0FC0_8002_A999_7C7F)
        .expect("0x0fc08002a9997c7f must open when the install is present");
    assert_eq!(guid::type_of(0x0FC0_8002_A999_7C7F), 0x040);
    let count = usize::from(reflection[0x3D]);
    let at = u64::from_le_bytes(reflection[0x08..0x10].try_into().unwrap()) as usize;
    assert!(count >= 3 && at + count * 8 <= reflection.len());
    let mut found = Vec::new();
    for i in 0..count {
        let rec = at + i * 8;
        let hash = u32::from_le_bytes(reflection[rec..rec + 4].try_into().unwrap());
        if hash == 0xAD10_F602 {
            found.push((reflection[rec + 4], reflection[rec + 5]));
        }
    }
    assert_eq!(found, vec![(2, 2)], "basecolor resource record changed");
}

/// A deterministic slice of the install's material data: the first 1,200
/// sorted type-`0B3` GUIDs. Every record run must fit, every GUID must be a
/// texture or one of its `0x0F1` aliases, and flags must be one of the two
/// observed values. This is **not** the full census; that is the ignored test
/// below.
#[test]
fn material_data_records_in_the_first_1200_are_stable() {
    let Some(data) = open() else {
        return;
    };
    let mut guids = data.guids_of_type(0x0B3);
    guids.sort_unstable();
    let total_assets = guids.len();
    let sample = 1_200.min(total_assets);
    let mut opened = 0usize;
    let mut payloads = 0usize;
    let mut records_total = 0usize;
    let mut aliases = 0usize;
    let mut with_flags = 0usize;
    for guid in guids.iter().take(sample) {
        let md = data
            .open_asset(*guid)
            .unwrap_or_else(|e| panic!("sample asset 0x{guid:016x} must open: {e}"));
        opened += 1;
        assert!(md.len() >= 0x40, "0x{guid:016x} shorter than the header");
        let count = usize::from(md[0x3C]);
        if count == 0 {
            continue;
        }
        let offset = i64::from_le_bytes(md[0x10..0x18].try_into().unwrap());
        assert!(
            offset > 0 && offset as usize + count * 16 <= md.len(),
            "0x{guid:016x}: count {count} at offset {offset} does not fit {} bytes",
            md.len()
        );
        payloads += 1;
        for record in texture_records(&md) {
            let t = guid::type_of(record.texture);
            assert!(
                t == 0x004 || t == 0x0F1,
                "0x{guid:016x} record type 0x{t:03x}"
            );
            if t == 0x0F1 {
                aliases += 1;
            }
            assert!(record.input != 0, "0x{guid:016x} record has no input hash");
            assert!(
                record.flags == 0 || record.flags == 0x8000,
                "0x{guid:016x} flags 0x{:08x}",
                record.flags
            );
            if record.flags != 0 {
                with_flags += 1;
            }
        }
        records_total += count;
    }
    assert_eq!(opened, sample, "the sample is exactly {sample} assets");
    eprintln!(
        "scope: first {sample} of {total_assets} type-0B3 assets; {payloads} with records, \
         {records_total} records, {aliases} 0x0F1 aliases, {with_flags} flag 0x8000"
    );
}

/// The full registry census. The counts are the install's, not a projection
/// from the sample: 121,035 registered type-`0B3` assets, 11 unreadable, 1,817
/// with an empty texture array, 119,207 with records and 693,111 records.
#[test]
#[ignore = "full install scan: 121,035 assets, about 6 minutes in release"]
fn full_install_material_data_census() {
    let Some(data) = open() else {
        return;
    };
    let mut guids = data.guids_of_type(0x0B3);
    guids.sort_unstable();
    assert_eq!(guids.len(), 121_035, "registered type-0B3 assets");

    let mut unreadable = 0usize;
    let mut short = 0usize;
    let mut empty = 0usize;
    let mut payloads = 0usize;
    let mut records = 0usize;
    let mut type004 = 0usize;
    let mut type0f1 = 0usize;
    let mut flags0 = 0usize;
    let mut flags8000 = 0usize;
    let mut zero_hash = 0usize;
    let mut alias_payloads = 0usize;

    for asset in &guids {
        let md = match data.open_asset(*asset) {
            Ok(md) => md,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        if md.len() < 0x40 {
            short += 1;
            continue;
        }
        let count = usize::from(md[0x3C]);
        if count == 0 {
            empty += 1;
            continue;
        }
        let offset = i64::from_le_bytes(md[0x10..0x18].try_into().unwrap());
        assert!(
            offset > 0 && offset as usize + count * 16 <= md.len(),
            "0x{asset:016x}: count {count} at offset {offset} does not fit {} bytes",
            md.len()
        );
        payloads += 1;
        let mut has_alias = false;
        for record in texture_records(&md) {
            records += 1;
            match guid::type_of(record.texture) {
                0x004 => type004 += 1,
                0x0F1 => {
                    type0f1 += 1;
                    has_alias = true;
                }
                other => panic!("0x{asset:016x} record type 0x{other:03x}"),
            }
            match record.flags {
                0 => flags0 += 1,
                0x8000 => flags8000 += 1,
                other => panic!("0x{asset:016x} flags 0x{other:08x}"),
            }
            if record.input == 0 {
                zero_hash += 1;
            }
        }
        if has_alias {
            alias_payloads += 1;
        }
    }

    eprintln!(
        "full census: {payloads} payloads, {records} records, {type004} type-004, \
         {type0f1} type-0F1 in {alias_payloads} payloads, flags 0={flags0} 0x8000={flags8000}, \
         empty={empty}, unreadable={unreadable}, short={short}, zero_hash={zero_hash}"
    );
    assert_eq!(unreadable, 11, "registered assets that do not open");
    assert_eq!(short, 0, "payloads shorter than the header");
    assert_eq!(empty, 1_817, "payloads with a zero texture count");
    assert_eq!(payloads, 119_207, "payloads with records");
    assert_eq!(records, 693_111, "texture records");
    assert_eq!(type004, 693_028, "type-0x004 records");
    assert_eq!(type0f1, 83, "type-0x0F1 alias records");
    assert_eq!(alias_payloads, 73, "payloads carrying an alias");
    assert_eq!(flags0, 678_297, "records with flags 0");
    assert_eq!(flags8000, 14_814, "records with flags 0x8000");
}

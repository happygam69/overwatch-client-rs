//! The material chain: a placement's *look* → shared materials → textures.
//!
//! This is the route that closes a model's surface to real texture data. The
//! model's own `MDLC` material list does **not** resolve as assets (measured --
//! `docs/formats/materials.md`), but the map does not rely on it: every model
//! placement carries a second reference, its *look*, and that is a
//! `STUModelLook` asset naming the materials the model is drawn with.
//!
//! ```text
//! placement.look            0x01A   STUModelLook        (type hash 0x5207484B)
//!   field 0xBAFDAFBA                STUModelMaterial[]  (inline, 24 B each)
//!     field 0x33E51FDC              m_material          a type-0x008 asset
//!     field 0xDC05EA3B              MaterialID          the slot the model names
//! material                  0x008   teMaterial          four GUIDs:
//!                                     +0x00 shader group (0x085)
//!                                     +0x08 shader source (0x088)
//!                                     +0x10 (0x03A)
//!                                     +0x18 material data (0x0B3)
//! material data             0x0B3   teMaterialData      header, then
//!                                         TextureCount × Texture at TextureOffset
//!                                     each Texture = GUID (0x004, or a 0x0F1
//!                                     texture alias), the zlib-CRC32 of its
//!                                     shader input name, and four flag bytes
//! ```
//!
//! The record layout and the count are read from the client. The material-data
//! load callback `FUN_141b1a7b0` (job 28, `MATERIAL DATA`, registered in
//! `FUN_141b19160`) fixes the payload's seven base-relative offsets; it reads
//! the byte counts +0x3D and +0x3E for the two pointer arrays at +0x18 and
//! +0x20. The texture count is the low byte of the packed u32 at +0x3C and the
//! record stride is 16: the texture-set consumer `FUN_141f1ca80` reads
//! `byte [payload+0x3C]`, records at `payload+[payload+0x10] + i*16`, compares
//! the u32 at record+8 against a name hash and copies the GUID at record+0.
//! `docs/formats/materials.md` records the addresses and the evidence class.
//!
//! The look and the two identifiers come from the reference checkout's
//! definitions (`TankLib/STU/Types/STUModelLook.cs`, `STUModelMaterial.cs`,
//! `teMaterial.cs`, `teMaterialData.cs`); they are used as a *hypothesis* and
//! each step is checked against the shipped bytes before it is trusted -- the
//! instance hash `0x5207484B` was confirmed by finding it in a real look asset
//! before this module existed (`ow-tank-look --walk 0x0980000000000156`).

use crate::{guid, GameData};

/// `STUModelLook`'s type hash, from the reference and confirmed in the bytes.
pub const MODEL_LOOK: u32 = 0x5207484b;
/// `STUModelMaterial`'s type hash.
pub const MODEL_MATERIAL: u32 = 0x494b66c1;
/// The look's inline array of materials.
const MATERIALS: u32 = 0xbafdafba;
/// `STUModelMaterial::m_material` -- the material asset.
const MATERIAL_REF: u32 = 0x33e51fdc;
/// `STUModelMaterial::m_DC05EA3B` -- the material's id, used as a slot key.
const MATERIAL_ID: u32 = 0xdc05ea3b;

/// The material references a look carries.
pub const MATERIAL_TYPE: u16 = 0x008;
/// Material data: the texture list.
pub const MATERIAL_DATA_TYPE: u16 = 0x0b3;

/// One entry of a look: which material asset, and the id the model names it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookMaterial {
    pub material: u64,
    pub id: u64,
}

/// The materials a look names, in the order the look stores them.
///
/// An unreadable or absent look is not an error the caller has to handle
/// specially: it returns an empty list, which is what "no materials known"
/// means everywhere above this.
pub fn materials_of_look(data: &GameData, look: u64) -> Result<Vec<LookMaterial>, crate::Error> {
    let stu = data.stu_of(look)?;
    let Some(instance) = stu.instance(MODEL_LOOK) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (bag, at) in stu.inline_elements(instance, MATERIALS) {
        let mut material = 0u64;
        let mut id = 0u64;
        for field in stu.layout_at(at, bag) {
            match field.hash {
                // The reference types this as a 16-byte
                // `teStructuredDataAssetRef<ulong>` and the field holds the
                // **GUID first**, with the padding slot after it. (Its array
                // reader takes padding first, but that path is for arrays whose
                // *elements* are asset refs; here the elements are instances and
                // this field is read the ordinary way.) Checked against the
                // bytes: the first qword decodes to type 0x008 with a small
                // index, the second to no type in the table at all.
                MATERIAL_REF => {
                    material = stu
                        .asset_ref_at(at + field.offset, MATERIAL_REF)
                        .unwrap_or(0);
                }
                MATERIAL_ID => id = stu.u64_at(at + field.offset).unwrap_or(0),
                _ => {}
            }
        }
        if material != 0 {
            out.push(LookMaterial { material, id });
        }
    }
    Ok(out)
}

/// The texture GUIDs a material names, from its material data.
pub fn textures_of_material(data: &GameData, material: u64) -> Result<Vec<u64>, crate::Error> {
    Ok(textures_of_material_data(&material_data_of(
        data, material,
    )?))
}

/// One texture record of a material-data payload.
///
/// The layout is read from the client's texture-set consumer `FUN_141f1ca80`:
/// a record is 16 bytes, `texture` is the asset GUID at `+0`, `input` is the
/// u32 at `+8` and `flags` is the four bytes at `+12`. The consumer compares
/// only the low u32 at `+8` and copies the GUID; it never reads `flags`.
/// Install observation over the full registry (119,207 payloads with records,
/// 693,111 records): `flags` is `0` (678,297) or `0x0000_8000` (14,814); the
/// GUID is type `0x004` in 693,028 records and type `0x0F1` in 83 texture-alias
/// records (73 payloads). The consumer copies either GUID without a type test.
/// The `0x80` sits at `+13`, so the old "flag byte at +12" wording counted the
/// wrong byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialTexture {
    pub texture: u64,
    /// The zlib CRC32 of the shader input name, as [`crc32`] computes it.
    pub input: u32,
    /// The four bytes at `+12`; not read by the client's texture-set consumer.
    pub flags: u32,
}

/// Texture-record stride: `FUN_141f1ca80` indexes `i * 0x10`.
pub const TEXTURE_RECORD_SIZE: usize = 16;
/// The texture-array offset, an i64 relative to the payload start (`+0x10`).
/// The load callback `FUN_141b1a7b0` rewrites it to `payload + offset`.
pub const TEXTURE_OFFSET_FIELD: usize = 0x10;
/// The packed count word at `+0x3C`.
///
/// Its low byte is the texture count (`FUN_141f1ca80` reads that byte), the
/// next byte is the count of the pointer array at `+0x18` and the third is the
/// count of the array at `+0x20`; the loader reads the last two as bytes.
pub const TEXTURE_COUNT_FIELD: usize = 0x3C;
/// Smallest payload that can carry the header fields this module reads.
pub const MATERIAL_DATA_HEADER: usize = 0x40;

/// zlib CRC32, the client's name hash.
///
/// `FUN_141b52ca0(ptr, len, initial)` is the standard reflected CRC32
/// (polynomial `0xEDB88320`, table at `0x143283ce0`) with the usual `~initial`
/// and final complement; `FUN_141bb9810` is the lazy string wrapper that calls
/// it with initial 0 over exactly `len` bytes. There is no NUL terminator in
/// the hash and the name is case-sensitive. This is the hash in a material
/// texture record: CRC32 of `b"basecolor"` is `0xAD10F602`, the record hash in
/// the pinned material below.
pub fn crc32_seeded(bytes: &[u8], initial: u32) -> u32 {
    let mut crc = !initial;
    for &byte in bytes {
        crc = (crc >> 8) ^ CRC32_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize];
    }
    !crc
}

/// [`crc32_seeded`] with the client's string-wrapper seed, 0.
pub fn crc32(bytes: &[u8]) -> u32 {
    crc32_seeded(bytes, 0)
}

/// The client's hash of one shader input name.
pub fn input_hash(name: &str) -> u32 {
    crc32(name.as_bytes())
}

const CRC32_POLYNOMIAL: u32 = 0xEDB8_8320;

const fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                CRC32_POLYNOMIAL ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = crc32_table();

/// The texture records of a material-data payload, exactly as stored.
///
/// The count is the byte at `+0x3C` and the offset the i64 at `+0x10`,
/// relative to the payload start. A payload that cannot hold its own array is
/// refused rather than read past its end; the client's consumer has no bounds
/// check because the load path has already validated the asset, so this is the
/// one place a caller must be protected.
pub fn texture_records(md: &[u8]) -> Vec<MaterialTexture> {
    if md.len() < MATERIAL_DATA_HEADER {
        return Vec::new();
    }
    let count = usize::from(md[TEXTURE_COUNT_FIELD]);
    let offset = i64::from_le_bytes(
        md[TEXTURE_OFFSET_FIELD..TEXTURE_OFFSET_FIELD + 8]
            .try_into()
            .unwrap(),
    );
    if count == 0 || offset <= 0 {
        return Vec::new();
    }
    let Some(end) = (offset as usize).checked_add(count * TEXTURE_RECORD_SIZE) else {
        return Vec::new();
    };
    if end > md.len() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = offset as usize + i * TEXTURE_RECORD_SIZE;
        out.push(MaterialTexture {
            texture: u64::from_le_bytes(md[at..at + 8].try_into().unwrap()),
            input: u32::from_le_bytes(md[at + 8..at + 12].try_into().unwrap()),
            flags: u32::from_le_bytes(md[at + 12..at + 16].try_into().unwrap()),
        });
    }
    out
}

/// The texture bound to one input-name hash in a material-data payload.
///
/// The client's texture-set consumer walks the records in order and takes the
/// first hit for each expected hash (`FUN_141f1ca80`). A duplicate hash
/// therefore resolves to the first record, as it does there.
pub fn texture_of_hash_in_data(md: &[u8], input: u32) -> Option<u64> {
    texture_records(md)
        .into_iter()
        .find(|record| record.input == input)
        .map(|record| record.texture)
}

/// The texture bound to a shader input name, hashed with [`crc32`].
pub fn texture_of_input_in_data(md: &[u8], name: &str) -> Option<u64> {
    texture_of_hash_in_data(md, input_hash(name))
}

/// Texture GUIDs in a material-data payload.
///
/// The client consumer copies the GUID without a type test, so this wrapper
/// returns every record's GUID in array order. A payload with an impossible
/// count/offset yields no records at all.
pub fn textures_of_material_data(md: &[u8]) -> Vec<u64> {
    texture_records(md)
        .into_iter()
        .map(|record| record.texture)
        .collect()
}

/// The material-data payload one material asset names at `+0x18`.
///
/// An absent material is an error (`open_asset`); a material header too short
/// for the fourth GUID or a fourth reference that is not type `0x0B3` gives an
/// empty payload, which is what "this material names no data" means here.
pub fn material_data_of(data: &GameData, material: u64) -> Result<Vec<u8>, crate::Error> {
    let bytes = data.open_asset(material)?;
    if bytes.len() < 32 {
        return Ok(Vec::new());
    }
    // The material header is four little-endian GUIDs; the fourth is the
    // material data. Not obfuscated -- this is a plain structure, not a STU
    // field, which is why the reference reads it with a plain struct read.
    let data_ref = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
    if data_ref == 0 || guid::type_of(data_ref) != MATERIAL_DATA_TYPE {
        return Ok(Vec::new());
    }
    data.open_asset(data_ref)
}

/// The texture records a material names, from its material data.
pub fn texture_records_of_material(
    data: &GameData,
    material: u64,
) -> Result<Vec<MaterialTexture>, crate::Error> {
    Ok(texture_records(&material_data_of(data, material)?))
}

/// The texture a material binds to one input-name hash.
///
/// The record's GUID is returned as stored, so it may be a `0x0F1` texture
/// alias; the client's consumer copies either without a type test.
pub fn texture_of_hash(data: &GameData, material: u64, input: u32) -> Option<u64> {
    let md = material_data_of(data, material).ok()?;
    texture_of_hash_in_data(&md, input)
}

/// The texture a material binds to a shader input name (`basecolor`, ...).
///
/// The record's GUID is returned as stored, so it may be a `0x0F1` texture
/// alias; the client's consumer copies either without a type test.
///
/// The binding is by name hash, as the client's texture-set consumer does it:
/// the record's u32 at `+8` is compared with the expected input hash and the
/// record's GUID is taken. Which names a given material is *expected* to carry
/// is the consumer's own table; this lookup answers only what the material
/// actually stores.
pub fn texture_of_input(data: &GameData, material: u64, name: &str) -> Option<u64> {
    texture_of_hash(data, material, input_hash(name))
}

/// A material's texture list, as `(guid, stored format byte)`.
///
/// The list holds *several* maps per material — measured on Tracer's goggles:
/// a BC5 two-channel pair (mean RGB 126, 126, 0 — a normal or mask map), a BC7
/// whose colour channels are all zero (an alpha/mask map) and a flat-grey BC1.
/// So "the first texture" is **not** the base colour, which is why the raw
/// first-entry rule is replaced by the sRGB rule below.
pub fn textures_with_format(
    data: &GameData,
    material: u64,
) -> Result<Vec<(u64, u8)>, crate::Error> {
    let guids = textures_of_material(data, material)?;
    Ok(guids
        .into_iter()
        .map(|g| {
            let format = data
                .open_asset(g)
                .ok()
                .and_then(|b| b.get(3).copied())
                .unwrap_or(0);
            (g, format)
        })
        .collect())
}

/// Is a stored format byte an **sRGB** (colour) format?
///
/// This is derived through the client's own format tables, not a second list:
/// [`crate::texture::is_srgb_format`] resolves the stored engine byte to its
/// DXGI id and answers true for the sRGB ids — `R8G8B8A8_UNORM_SRGB` (29),
/// `BC1_UNORM_SRGB` (72), `BC2_UNORM_SRGB` (75), `BC3_UNORM_SRGB` (78),
/// `B8G8R8A8_UNORM_SRGB` (91) and `BC7_UNORM_SRGB` (99). The stored bytes the
/// install's material lists use are the five non-BGRA ones — 30, 73, 76, 79 and
/// 100 — which is the set this function used to hard-code; the predicate also
/// accepts the BGRA sRGB bytes 0x5C/0x5E. Colour maps (albedo, emissive) are
/// stored as sRGB and data maps (normals, roughness, masks) are not, which is
/// what makes this a *principled* choice of base colour rather than a guess at
/// the array order.
pub fn is_srgb_format(stored: u8) -> bool {
    crate::texture::is_srgb_format(stored)
}

/// The texture a material uses as base colour: its first **sRGB** entry.
///
/// This is the viewer's rule, not the client's. The client selects textures by
/// the name hash stored in the record (see [`texture_of_input`]); the generic
/// "which input is albedo" rule that would let this function answer the same
/// question has not been read, so it stays an explicit heuristic and is listed
/// as open in `docs/formats/materials.md`. It now walks the exact record parse
/// ([`texture_records`]), including the count byte and 16-byte stride.
///
/// `None` when it names none, and that is deliberate: painting a material's
/// data maps as colour is worse than leaving the surface on the placeholder,
/// because a normal map rendered as albedo looks like a texture while being
/// wrong. Measured on Tracer's three readable materials — a BC5 pair (mean RGB
/// 126, 126, 0), a BC7 whose colour is black, and two dark-grey BC1s — none of
/// them an albedo, which is why the hero's surfaces all end up on the
/// placeholder on this install.
pub fn base_texture_of_material(data: &GameData, material: u64) -> Option<u64> {
    textures_with_format(data, material)
        .ok()?
        .into_iter()
        .find(|(_, stored)| is_srgb_format(*stored))
        .map(|(g, _)| g)
}

/// The material asset one MDLC slot value names through a look, when the
/// naming is unambiguous.
///
/// A model's `MDLC` list holds 64-bit slot values and a look holds one entry
/// per slot id (`MaterialID`, field `0xDC05EA3B`); the two meet on that id.
/// Exactly one entry carrying the id gives its material; zero entries (an id
/// the look does not name) and two or more (the same id on different
/// materials) both give `None` — the second is the ambiguity the scene leaves
/// on the placeholder rather than guessing, and the caller reports it.
pub fn material_of_id(look_materials: &[LookMaterial], id: u64) -> Option<u64> {
    let mut found = None;
    for m in look_materials {
        if m.id != id {
            continue;
        }
        // The same id twice on the *same* material is a harmless duplicate;
        // the same id on *different* materials is a coin toss, declined.
        if found.is_some_and(|f| f != m.material) {
            return None;
        }
        found = Some(m.material);
    }
    found
}

/// Everything a caller needs to draw one mesh of a model with a look: the base
/// colour texture, if one is named.
///
/// The lookup is by index into the look's material list, which is what the
/// harness uses; a model whose material count disagrees with the look's is
/// left untextured rather than mismatched. The texture is the material's base
/// colour ([`base_texture_of_material`]), not its first entry: the first entry
/// is often a data map, which paints wrong as colour.
pub fn base_texture_of(data: &GameData, look: u64, slot: usize) -> Option<u64> {
    let materials = materials_of_look(data, look).ok()?;
    let m = materials.get(slot)?;
    base_texture_of_material(data, m.material)
}

/// The base-colour texture for one MDLC slot value: the unambiguous join of
/// the model's value to the look's id to the material's texture list.
///
/// `None` covers every way of not knowing: the look unreadable, the id
/// unnamed or ambiguously named ([`material_of_id`]), the material unreadable
/// on this install, or no sRGB entry in its list.
pub fn base_texture_of_id(data: &GameData, look: u64, id: u64) -> Option<u64> {
    let materials = materials_of_look(data, look).ok()?;
    let material = material_of_id(&materials, id)?;
    base_texture_of_material(data, material)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A material-data payload with `count` texture records at offset 0x60.
    ///
    /// The two pointer-array counts (+0x3D/+0x3E) are deliberately non-zero:
    /// the texture count is only the low byte of the packed word at +0x3C, and
    /// a parser that used the whole u32 would either overrun or read the wrong
    /// records.
    fn material_data(records: &[(u64, u32, u32)]) -> Vec<u8> {
        let mut md = vec![0u8; 0x60 + TEXTURE_RECORD_SIZE * records.len()];
        md[0x00..0x08].copy_from_slice(&0x48i64.to_le_bytes());
        md[0x10..0x18].copy_from_slice(&0x60i64.to_le_bytes());
        md[0x38] = 9; // parameter count, low byte of its own packed word
        md[0x3C] = records.len() as u8;
        md[0x3D] = 7;
        md[0x3E] = 5;
        for (i, (texture, input, flags)) in records.iter().enumerate() {
            let at = 0x60 + i * TEXTURE_RECORD_SIZE;
            md[at..at + 8].copy_from_slice(&texture.to_le_bytes());
            md[at + 8..at + 12].copy_from_slice(&input.to_le_bytes());
            md[at + 12..at + 16].copy_from_slice(&flags.to_le_bytes());
        }
        md
    }

    #[test]
    fn crc32_matches_zlib_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"basecolor"), 0xAD10_F602);
        // Case matters: "BaseColor" is a different hash.
        assert_eq!(crc32(b"BaseColor"), 0xEE20_5BA5);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // The client's `initial` is the running CRC, complemented on entry and
        // return exactly as zlib's crc32(data, previous) does.
        assert_eq!(crc32_seeded(b"123", 0), crc32(b"123"));
        assert_eq!(crc32_seeded(b"456789", crc32(b"123")), crc32(b"123456789"));
        assert_eq!(input_hash("basecolor"), 0xAD10_F602);
    }

    #[test]
    fn reads_records_with_the_count_byte_and_16_byte_stride() {
        let records = [
            (0x0C00_0000_0000_0E49u64, 0xAD10_F602u32, 0u32),
            (0x0C00_0000_0000_0E4A, 0x20AF_06CE, 0x8000),
        ];
        let md = material_data(&records);
        assert_eq!(
            u32::from_le_bytes(md[0x3C..0x40].try_into().unwrap()),
            0x0005_0702,
            "the packed word's other bytes are counts of other arrays"
        );
        let parsed = texture_records(&md);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].texture, 0x0C00_0000_0000_0E49);
        assert_eq!(parsed[0].input, 0xAD10_F602);
        assert_eq!(parsed[0].flags, 0);
        assert_eq!(parsed[1].texture, 0x0C00_0000_0000_0E4A);
        assert_eq!(parsed[1].input, 0x20AF_06CE);
        assert_eq!(parsed[1].flags, 0x8000);
        assert_eq!(
            textures_of_material_data(&md),
            vec![0x0C00_0000_0000_0E49, 0x0C00_0000_0000_0E4A]
        );
    }

    #[test]
    fn the_parameter_array_is_not_read_as_textures() {
        let records = [(0x0C00_0000_0000_0E49u64, 0xAD10_F602u32, 0u32)];
        let mut md = material_data(&records);
        // Three 8-byte parameter records live at +0x48, before the textures;
        // the parameter count byte at +0x38 is already non-zero in this
        // synthetic header, so a parser that mistook it for the texture count
        // would read past the end.
        for i in 0..3 {
            md[0x48 + i * 8..0x4C + i * 8]
                .copy_from_slice(&(0x1000_0000u32 + i as u32).to_le_bytes());
        }
        let parsed = texture_records(&md);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].texture, 0x0C00_0000_0000_0E49);
    }

    #[test]
    fn looks_up_by_input_hash_and_name() {
        let records = [
            (0x0C00_0000_0000_0E49u64, 0xAD10_F602u32, 0u32),
            (0x0C00_0000_0000_0E4A, 0x20AF_06CE, 0u32),
        ];
        let md = material_data(&records);
        assert_eq!(
            texture_of_hash_in_data(&md, 0xAD10_F602),
            Some(0x0C00_0000_0000_0E49)
        );
        assert_eq!(
            texture_of_input_in_data(&md, "basecolor"),
            Some(0x0C00_0000_0000_0E49)
        );
        assert_eq!(texture_of_input_in_data(&md, "BaseColor"), None);
        assert_eq!(texture_of_hash_in_data(&md, 0xDEAD_BEEF), None);
        // The client takes the first record for an expected hash.
        let duplicate = material_data(&[
            (0x0C00_0000_0000_0E49u64, 0xAD10_F602u32, 0u32),
            (0x0C00_0000_0000_0E4A, 0xAD10_F602, 0u32),
        ]);
        assert_eq!(
            texture_of_hash_in_data(&duplicate, 0xAD10_F602),
            Some(0x0C00_0000_0000_0E49)
        );
    }

    #[test]
    fn refuses_impossible_offsets_and_counts() {
        let records = [(0x0C00_0000_0000_0E49u64, 0xAD10_F602u32, 0u32)];

        // A negative offset is not a pointer the loader can produce.
        let mut md = material_data(&records);
        md[0x10..0x18].copy_from_slice(&(-1i64).to_le_bytes());
        assert!(texture_records(&md).is_empty());

        // A stored zero is the loader's null: there is no array to walk.
        let mut md = material_data(&records);
        md[0x10..0x18].copy_from_slice(&0i64.to_le_bytes());
        assert!(texture_records(&md).is_empty());

        // An offset at EOF leaves no room for a record.
        let mut md = material_data(&records);
        let eof = md.len() as i64;
        md[0x10..0x18].copy_from_slice(&eof.to_le_bytes());
        assert!(texture_records(&md).is_empty());

        // A count whose array runs past EOF is refused, not clipped.
        let mut md = material_data(&records);
        md[0x3C] = 2;
        assert!(texture_records(&md).is_empty());

        // A payload shorter than the header cannot carry a count/offset.
        assert!(texture_records(&[0u8; 0x3C]).is_empty());
        assert!(texture_records(&[]).is_empty());
    }

    #[test]
    fn a_zero_count_is_empty_even_with_an_offset() {
        let mut md = vec![0u8; 0x60];
        md[0x10..0x18].copy_from_slice(&0x60i64.to_le_bytes());
        assert!(texture_records(&md).is_empty());
        assert!(textures_of_material_data(&md).is_empty());
    }

    #[test]
    fn the_id_join_needs_exactly_one_material() {
        let look = vec![
            LookMaterial {
                material: 0x11,
                id: 0xA,
            },
            LookMaterial {
                material: 0x22,
                id: 0xB,
            },
            // The same entry twice is a duplicate, not a disagreement.
            LookMaterial {
                material: 0x22,
                id: 0xB,
            },
        ];
        assert_eq!(material_of_id(&look, 0xA), Some(0x11));
        assert_eq!(material_of_id(&look, 0xB), Some(0x22));
        assert_eq!(material_of_id(&look, 0xC), None);
        // ... while the same id on two *different* materials is ambiguous.
        let mut clash = look.clone();
        clash.push(LookMaterial {
            material: 0x33,
            id: 0xB,
        });
        assert_eq!(material_of_id(&clash, 0xB), None);
        assert_eq!(material_of_id(&clash, 0xA), Some(0x11));
    }
}

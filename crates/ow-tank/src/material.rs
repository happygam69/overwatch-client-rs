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
//!                                     each Texture = GUID (0x004) + name hash
//! ```
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
    let md = data.open_asset(data_ref)?;
    Ok(textures_of_material_data(&md))
}

/// Texture GUIDs in a material-data payload.
///
/// The header is four i64 offsets, then padding, then counts: the texture count
/// is the byte at +60 and the texture offset the i64 at +16. Each record is a
/// GUID followed by a CRC32 of the input name and a flag byte; the stride is
/// taken from the data rather than assumed, because the packed structure's tail
/// padding is not documented (a record whose name hash is 0 would otherwise
/// pass as the next texture's GUID).
pub fn textures_of_material_data(md: &[u8]) -> Vec<u64> {
    let Some(count) = md.get(60).copied() else {
        return Vec::new();
    };
    let offset = md
        .get(16..24)
        .map(|b| i64::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0);
    if count == 0 || offset <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Try the two plausible strides and keep the one whose records all look
    // like textures.
    for stride in [16usize, 12] {
        let mut candidate = Vec::new();
        for i in 0..count as usize {
            let at = offset as usize + i * stride;
            let Some(g) = md.get(at..at + 8) else {
                candidate.clear();
                break;
            };
            let g = u64::from_le_bytes(g.try_into().unwrap());
            if guid::type_of(g) != crate::guid::types::TEXTURE {
                candidate.clear();
                break;
            }
            candidate.push(g);
        }
        if candidate.len() == count as usize {
            return candidate;
        }
    }
    std::mem::take(&mut out)
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
/// The client stores one value higher from DXGI id 26 up, so the sRGB variants
/// land on these bytes: `R8G8B8A8_UNORM_SRGB` (30), `BC1_UNORM_SRGB` (73),
/// `BC2_UNORM_SRGB` (76), `BC3_UNORM_SRGB` (79) and `BC7_UNORM_SRGB` (100).
/// Colour maps (albedo, emissive) are stored as sRGB and data maps (normals,
/// roughness, masks) are not, which is what makes this a *principled* choice of
/// base colour rather than a guess at the array order.
pub fn is_srgb_format(stored: u8) -> bool {
    matches!(stored, 30 | 73 | 76 | 79 | 100)
}

/// The texture a material uses as base colour: its first **sRGB** entry.
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

    /// A material-data header with two texture records at offset 64, written
    /// the way the shipped files are: GUID, CRC32, flag, padded to 16.
    fn material_data(guids: &[u64], stride: usize) -> Vec<u8> {
        let mut md = vec![0u8; 64 + stride * guids.len()];
        md[16..24].copy_from_slice(&64i64.to_le_bytes());
        md[60] = guids.len() as u8;
        for (i, g) in guids.iter().enumerate() {
            let at = 64 + i * stride;
            md[at..at + 8].copy_from_slice(&g.to_le_bytes());
            md[at + 8..at + 12].copy_from_slice(&0xdeadbeefu32.to_le_bytes());
            md[at + 12] = 1;
        }
        md
    }

    #[test]
    fn reads_textures_at_the_padded_stride() {
        let guids = [0x0c00000000000003u64, 0x0c0000000000000a];
        let md = material_data(&guids, 16);
        assert_eq!(textures_of_material_data(&md), guids.to_vec());
    }

    #[test]
    fn refuses_records_that_are_not_textures() {
        // The same layout, but the GUIDs are not texture-typed: a stride guess
        // would happily read them, so the type check is what stops it.
        let guids = [0x0c00000000000003u64, 0x0980000000000156];
        let md = material_data(&guids, 16);
        assert!(textures_of_material_data(&md).is_empty());
    }

    #[test]
    fn an_empty_material_data_is_not_an_error() {
        let mut md = vec![0u8; 64];
        md[60] = 0;
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

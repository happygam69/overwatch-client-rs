//! Two real install textures, one sRGB colour map and one linear data map, prove
//! that the viewer's upload colour space comes from the asset's stored format
//! byte and not from a material or a renderer-wide rule.
//!
//! The assets are pinned by GUID only because both were read out of this
//! install's registry and their stored bytes checked with `ow-tank-tex`
//! (`0x0C0000000000E349` is byte 100, BC7_UNORM_SRGB; `0x0C000000000064D2` is
//! byte 81, BC4_UNORM). `0x0C000000000064D2` is one of the six pinned assets in
//! `docs/formats/materials.md` § "The six pinned assets";
//! `0x0C0000000000E349` is a pinned material texture in
//! `crates/ow-tank/tests/material_real.rs`, not in that table. The test does not
//! choose content: it asks the client's store for those two assets and skips
//! only when the install is absent.
//!
//! Read-only.
//!
//! ```text
//! cargo test --release -p ow-scene --test texture_colour_space -- --nocapture
//! ```

use ow_tank::keytable::Sources;
use ow_tank::{texture, GameData, ManifestSelector};

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

/// Decode one pinned texture. The install is known to be present here, so a
/// missing or undecodable pinned asset is a failure, not a skip.
fn decode(data: &GameData, guid: u64) -> texture::Texture {
    let bytes = data.open_asset(guid).unwrap_or_else(|e| {
        panic!("pinned texture 0x{guid:016x} must open when the install is present: {e}")
    });
    let loader = |g: u64| data.open_asset(g).ok();
    texture::parse_asset(&bytes, guid, &loader).unwrap_or_else(|e| {
        panic!("pinned texture 0x{guid:016x} must decode when the install is present: {e}")
    })
}

#[test]
fn scene_textures_take_their_colour_space_from_the_stored_format() {
    let Some(data) = open() else {
        return;
    };

    // 0x0C0000000000E349: stored 0x64 -> BC7_UNORM_SRGB (DXGI 99).
    let colour = decode(&data, 0x0c0000000000e349);
    assert_eq!(
        colour.format_byte, 0x64,
        "the pinned sRGB asset's stored byte"
    );
    assert_eq!(ow_tank::texture::dxgi_of(colour.format_byte), 99);
    let colour = ow_scene::SceneTexture::decoded(0x0c0000000000e349, &colour);
    assert!(colour.srgb, "BC7_UNORM_SRGB is an sRGB asset");

    // 0x0C000000000064D2: stored 0x51 -> BC4_UNORM (DXGI 80), a linear map.
    let data_map = decode(&data, 0x0c000000000064d2);
    assert_eq!(
        data_map.format_byte, 0x51,
        "the pinned linear asset's stored byte"
    );
    assert_eq!(ow_tank::texture::dxgi_of(data_map.format_byte), 80);
    let data_map = ow_scene::SceneTexture::decoded(0x0c000000000064d2, &data_map);
    assert!(!data_map.srgb, "BC4_UNORM is a linear data map");
}

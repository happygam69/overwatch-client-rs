//! Uploading a decoded texture, and the bind group the surface shader samples.
//!
//! One bind group per texture (group 1), a 1×1 white entry for draws with no
//! readable material, and a plain linear sampler: the install's textures ship
//! no sampler of their own that this project can read.
//!
//! The upload writes **every** subresource the decoder produced, in the
//! client's copy order: `mip_level_count` and `depth_or_array_layers` come from
//! the levels, and each level is one `write_texture` at its own mip and layer.
//! The client's texture SRV view dimension and its cube-face order are not read
//! yet, so the bind group is still a 2D view of layer 0 — with the whole mip
//! chain present — rather than a cube view. Sampling is therefore face 0 as
//! before; the uploaded bytes are the client's layout.

use ow_scene::SceneTexture;

/// The joint palette buffer's capacity, in matrices. 256 covers every rig
/// read so far (the character models are 200 bones, the view models 71); a rig
/// larger than this would need the buffer sized from the scene instead.
pub const MAX_JOINTS: usize = 256;

/// Group 2: the joint palette. Always bound, even for scenes with no skinned
/// draw, because slot 0 (the identity) is what the shader uses for them.
pub fn joint_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("joints"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// Upload a palette (already in the shader's column-major order) and bind it.
/// A palette shorter than [`MAX_JOINTS`] is padded with the identity, so an
/// out-of-range index in a malformed mesh cannot read another draw's bones.
pub fn joint_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    palette: &[[[f32; 4]; 4]],
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let mut data = Vec::with_capacity(MAX_JOINTS * 16);
    for m in palette.iter().take(MAX_JOINTS) {
        for row in m.iter() {
            data.extend_from_slice(row);
        }
    }
    while data.len() < MAX_JOINTS * 16 {
        let identity = glam::Mat4::IDENTITY.to_cols_array();
        data.extend_from_slice(&identity);
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("joints"),
        size: (MAX_JOINTS * 64) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&data));
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("joints"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, bind_group)
}

/// Group 1: a texture and its sampler.
pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("surface"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Upload every scene texture. The returned vector is indexed by
/// [`ow_scene::Draw::texture`], which is what makes a draw's texture choice a
/// one-line bind.
pub fn upload_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    textures: &[SceneTexture],
) -> Vec<wgpu::BindGroup> {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("surface"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    textures
        .iter()
        .map(|t| upload_one(device, queue, layout, &sampler, t))
        .collect()
}

/// The upload format and its bytes per pixel.
///
/// R16_UNORM (the client's DXGI 56) is uploaded as `R16Unorm`, two bytes per
/// pixel, when the device was created with
/// [`wgpu::Features::TEXTURE_FORMAT_16BIT_NORM`] and every level carries its
/// exact stored words. The format is the client's own DXGI 56 resource; the
/// `u16` words are staged unchanged, not converted to half floats. WebGPU 22
/// requires that feature to create the texture even though the format's sample
/// type is filterable float; without it the R16 asset falls back to the clamped
/// 8-bit view below.
///
/// A texture whose decoded levels carry exact half floats (BC6H,
/// R16G16B16A16_FLOAT, R16_FLOAT — the client's DXGI 95/96/10/54) is uploaded
/// as `Rgba16Float`, eight bytes per pixel, so the values reach the GPU
/// unchanged. The client's own resource is those DXGI ids; `Rgba16Float` is a
/// **viewer substitution** with the same four 16-bit float channels, not a
/// claim that the client creates that resource.
///
/// Every other texture is uploaded as 8-bit RGBA in the asset's colour space:
/// an sRGB asset is `Rgba8UnormSrgb`, so sampling linearises it the way the
/// client's own sRGB view does; a linear data map (normal, mask, height) is
/// `Rgba8Unorm` and sampled raw. The client picks the same DXGI id for its
/// resource and its texture SRV (`FUN_1426545d0`/`FUN_142653b90`), so this is
/// the asset's format, not a renderer-wide guess. The UINT families have no
/// exact path yet and take this clamped view (open).
fn upload_format(tex: &SceneTexture, has_16bit_norm: bool) -> (wgpu::TextureFormat, usize) {
    if has_16bit_norm && tex.has_exact_r16_unorm() {
        (wgpu::TextureFormat::R16Unorm, 2)
    } else if tex.has_exact_half() {
        (wgpu::TextureFormat::Rgba16Float, 8)
    } else if tex.srgb {
        (wgpu::TextureFormat::Rgba8UnormSrgb, 4)
    } else {
        (wgpu::TextureFormat::Rgba8Unorm, 4)
    }
}

/// The staging row pitch: `write_texture` wants rows padded to
/// `COPY_BYTES_PER_ROW_ALIGNMENT` (256 bytes), which is the same padding the
/// RGBA8 path has always used.
fn row_pitch(row_bytes: usize) -> usize {
    row_bytes.div_ceil(256) * 256
}

/// Copy one level's rows into a staging buffer whose rows are `pitch` bytes.
/// A short source leaves the remaining rows zero rather than reading past it.
fn stage_rows(staging: &mut [u8], source: &[u8], row_bytes: usize, rows: usize, pitch: usize) {
    for y in 0..rows {
        let at = y * row_bytes;
        let Some(row) = source.get(at..at + row_bytes) else {
            break;
        };
        staging[y * pitch..y * pitch + row_bytes].copy_from_slice(row);
    }
}

/// The exact rows for one upload format. The format choice and the byte source
/// are kept together so a two-byte texture can only read `r16_unorm`, an
/// eight-byte texture only `rgba16`, and the 8-bit path `rgba`.
fn level_upload_bytes(level: &ow_scene::SceneTextureLevel, bytes_per_pixel: usize) -> &[u8] {
    match bytes_per_pixel {
        2 => level
            .r16_unorm
            .as_deref()
            .map(bytemuck::cast_slice::<u16, u8>)
            .unwrap_or(&[]),
        8 => level
            .rgba16
            .as_deref()
            .map(bytemuck::cast_slice::<u16, u8>)
            .unwrap_or(&[]),
        _ => &level.rgba,
    }
}

/// `(mip_level_count, array_layers)` for the texture that holds `levels`.
///
/// The decoder lists every subresource, so the shape is the highest mip and
/// layer index plus one — the same counts the client's resource descriptor gets
/// (`0x1426545d0`: `MipLevels`, and `DepthOrArraySize = array_count * 6` for a
/// cube).
fn upload_shape(levels: &[ow_scene::SceneTextureLevel]) -> (u32, u32) {
    let mips = levels
        .iter()
        .map(|level| level.mip as u32 + 1)
        .max()
        .unwrap_or(1);
    let layers = levels
        .iter()
        .map(|level| level.surface as u32 + 1)
        .max()
        .unwrap_or(1);
    (mips, layers)
}

fn upload_one(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    tex: &SceneTexture,
) -> wgpu::BindGroup {
    // The constructors always produce a level list; this keeps a hand-built
    // SceneTexture (the tests) uploadable.
    let fallback;
    let levels: &[ow_scene::SceneTextureLevel] = if tex.levels.is_empty() {
        fallback = ow_scene::SceneTextureLevel {
            mip: 0,
            surface: 0,
            width: tex.width.max(1),
            height: tex.height.max(1),
            rgba: tex.rgba.clone(),
            rgba16: None,
            r16_unorm: None,
        };
        std::slice::from_ref(&fallback)
    } else {
        &tex.levels
    };

    let has_16bit_norm = device
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
    let (format, bytes_per_pixel) = upload_format(tex, has_16bit_norm);
    let (mip_level_count, layers) = upload_shape(levels);
    let (width, height) = (tex.width.max(1) as u32, tex.height.max(1) as u32);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("surface"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // `R16Unorm` for exact R16 words when the feature is present,
        // `Rgba16Float` for exact halves, otherwise 8-bit RGBA in the asset's
        // own colour space (`SceneTexture::srgb`, from its stored format byte)
        // — not a guess at this call site.
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // One write per decoded subresource, at its own mip level and layer. This
    // is the client's own block-run order (`0x141e296a0`: surface outer, mip
    // ascending); `write_texture` wants rows padded to 256 bytes.
    for level in levels {
        let level_width = level.width.max(1) as usize;
        let level_height = level.height.max(1) as usize;
        let unpadded = level_width * bytes_per_pixel;
        let padded = row_pitch(unpadded);
        let mut staging = vec![0u8; padded * level_height];
        // The byte source follows the format choice, so a level's rows are
        // always interpreted as the channels the texture was created with.
        let source = level_upload_bytes(level, bytes_per_pixel);
        stage_rows(&mut staging, source, unpadded, level_height, padded);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: level.mip as u32,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: level.surface as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &staging,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(level_height as u32),
            },
            wgpu::Extent3d {
                width: level_width as u32,
                height: level_height as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    // The client's SRV dimension/face order is open, so the shader keeps a 2D
    // view of layer 0 (all mip levels). It samples face 0 exactly as before;
    // the array's other layers are uploaded but not bound.
    let view = if layers > 1 {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("surface layer 0"),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_mip_level: 0,
            mip_level_count: Some(mip_level_count),
            base_array_layer: 0,
            array_layer_count: Some(1),
            ..Default::default()
        })
    } else {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("surface"),
            ..Default::default()
        })
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("surface"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_format_follows_exact_halves_and_the_asset_colour_space() {
        // Exact halves win over the colour-space bit: BC6H, RGBA16F and R16F
        // all carry them, and their values must not be clamped to bytes.
        let mut hdr = SceneTexture::white();
        hdr.srgb = false;
        hdr.levels[0].rgba16 = Some(vec![0; 4]);
        assert_eq!(
            upload_format(&hdr, true),
            (wgpu::TextureFormat::Rgba16Float, 8)
        );
        assert_eq!(
            upload_format(&hdr, false),
            (wgpu::TextureFormat::Rgba16Float, 8)
        );

        // No halves: the asset's own colour space decides, as before.
        assert_eq!(
            upload_format(&SceneTexture::white(), true),
            (wgpu::TextureFormat::Rgba8UnormSrgb, 4)
        );
        let mut linear = SceneTexture::white();
        linear.srgb = false;
        assert_eq!(
            upload_format(&linear, true),
            (wgpu::TextureFormat::Rgba8Unorm, 4)
        );
    }

    #[test]
    fn r16_unorm_upload_is_exact_only_with_the_16bit_norm_feature() {
        let mut r16 = SceneTexture::white();
        r16.srgb = false;
        r16.levels[0].r16_unorm = Some(vec![0x0001, 0x8000, 0xFFFE, 0xFFFF]);

        // The client's resource is DXGI 56 and the words are staged raw.
        assert_eq!(
            upload_format(&r16, true),
            (wgpu::TextureFormat::R16Unorm, 2)
        );
        // Without the optional feature wgpu cannot create `R16Unorm`; the
        // clamped linear 8-bit view remains the honest fallback.
        assert_eq!(
            upload_format(&r16, false),
            (wgpu::TextureFormat::Rgba8Unorm, 4)
        );
        // The exact words are not halves, so the half path must ignore them.
        assert!(!r16.has_exact_half());
    }

    #[test]
    fn row_pitch_is_256_for_both_pixel_sizes() {
        // R16_UNORM: 2 bytes per pixel.
        assert_eq!(row_pitch(2), 256);
        assert_eq!(row_pitch(256), 256);
        assert_eq!(row_pitch(258), 512);
        // RGBA8: 4 bytes per pixel.
        assert_eq!(row_pitch(4), 256);
        assert_eq!(row_pitch(256), 256);
        assert_eq!(row_pitch(260), 512);
        // RGBA16F: 8 bytes per pixel, so half as many pixels per 256-byte row.
        assert_eq!(row_pitch(8), 256);
        assert_eq!(row_pitch(256), 256);
        assert_eq!(row_pitch(264), 512);
    }

    #[test]
    fn r16_unorm_staging_is_the_stored_word_bytes_in_order() {
        let mut level = level(0, 0);
        let words = [0x0001u16, 0x8000, 0xFFFE, 0xFFFF];
        level.r16_unorm = Some(words.to_vec());
        let source = level_upload_bytes(&level, 2);
        assert_eq!(
            source,
            &[0x01, 0x00, 0x00, 0x80, 0xFE, 0xFF, 0xFF, 0xFF],
            "little-endian stored words, no half conversion and no clamp"
        );

        let mut staging = vec![0u8; row_pitch(4 * 2)];
        stage_rows(&mut staging, source, 4 * 2, 1, row_pitch(4 * 2));
        assert_eq!(&staging[..8], source);
        assert!(staging[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn stage_rows_keeps_eight_byte_pixels_bit_for_bit() {
        // 2x2 RGBA16F: 8 pixels x 4 halves = 32 half words, two 16-byte rows in
        // a 256-byte-pitch staging buffer.
        let halves: Vec<u16> = (1..=32).collect();
        let source: &[u8] = bytemuck::cast_slice(&halves);
        let mut staging = vec![0u8; row_pitch(2 * 8) * 2];
        stage_rows(&mut staging, source, 2 * 8, 2, row_pitch(2 * 8));

        assert_eq!(&staging[0..16], &source[0..16]);
        assert_eq!(&staging[256..272], &source[16..32]);
        assert!(staging[16..256].iter().all(|b| *b == 0));
        // The exact half words survive: no f32 round-trip, no clamp.
        assert_eq!(&staging[0..2], &0x0001u16.to_le_bytes());
    }

    #[test]
    fn rgba16_float_is_the_filterable_float_sample_the_layout_asks_for() {
        // `layout()` binds `TextureSampleType::Float { filterable: true }`.
        // WebGPU must report the 16-bit float format with that sample type and
        // without requiring a filtering feature, or the bind group would fail
        // validation at runtime.
        assert_eq!(
            wgpu::TextureFormat::Rgba16Float.sample_type(
                Some(wgpu::TextureAspect::All),
                Some(wgpu::Features::empty())
            ),
            Some(wgpu::TextureSampleType::Float { filterable: true })
        );
    }

    #[test]
    fn r16_unorm_is_filterable_but_requires_the_16bit_norm_feature() {
        // The existing layout binds `Float { filterable: true }`, which
        // `R16Unorm` satisfies. WebGPU still refuses to create the texture
        // unless the device enabled the optional feature, so the uploader
        // gates on `device.features()` before choosing it.
        assert_eq!(
            wgpu::TextureFormat::R16Unorm.sample_type(
                Some(wgpu::TextureAspect::All),
                Some(wgpu::Features::empty())
            ),
            Some(wgpu::TextureSampleType::Float { filterable: true })
        );
        assert!(wgpu::TextureFormat::R16Unorm
            .required_features()
            .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM));
    }

    fn level(mip: u8, surface: u16) -> ow_scene::SceneTextureLevel {
        ow_scene::SceneTextureLevel {
            mip,
            surface,
            width: 4,
            height: 4,
            rgba: vec![0; 4 * 4 * 4],
            rgba16: None,
            r16_unorm: None,
        }
    }

    #[test]
    fn upload_shape_covers_every_decoded_level_and_layer() {
        // An 8 mip x 6 layer cube: 48 subresources, 8 mips and 6 layers.
        let cube: Vec<_> = (0..6u16)
            .flat_map(|surface| (0..8u8).map(move |mip| level(mip, surface)))
            .collect();
        assert_eq!(upload_shape(&cube), (8, 6));
        // One mip, one layer.
        assert_eq!(upload_shape(&[level(0, 0)]), (1, 1));
        // The list is not assumed sorted: the maximum index wins.
        assert_eq!(upload_shape(&[level(2, 3), level(0, 0)]), (3, 4));
        assert_eq!(upload_shape(&[]), (1, 1));
    }
}

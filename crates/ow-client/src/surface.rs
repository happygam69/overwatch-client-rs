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

/// The upload view format for a texture, from the colour space its stored
/// format byte names.
///
/// An sRGB asset is uploaded as `Rgba8UnormSrgb`, so sampling linearises it the
/// way the client's own sRGB view does; a linear data map (normal, mask,
/// height) is uploaded as `Rgba8Unorm` and sampled raw. The client picks the
/// same DXGI id for its resource and its texture SRV
/// (`FUN_1426545d0`/`FUN_142653b90`), so this is the asset's format, not a
/// renderer-wide guess.
fn upload_format(srgb: bool) -> wgpu::TextureFormat {
    if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
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
        };
        std::slice::from_ref(&fallback)
    } else {
        &tex.levels
    };

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
        // sRGB assets are sampled as sRGB and linearised by the hardware;
        // linear data maps are sampled raw. The bit came from the asset's
        // stored format byte (`SceneTexture::srgb`), not from this call site.
        format: upload_format(tex.srgb),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // One write per decoded subresource, at its own mip level and layer. This
    // is the client's own block-run order (`0x141e296a0`: surface outer, mip
    // ascending); `write_texture` wants rows padded to 256 bytes.
    for level in levels {
        let level_width = level.width.max(1) as usize;
        let level_height = level.height.max(1) as usize;
        let unpadded = level_width * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let mut staging = vec![0u8; padded * level_height];
        for y in 0..level_height {
            let at = y * unpadded;
            let Some(row) = level.rgba.get(at..at + unpadded) else {
                break;
            };
            staging[y * padded..y * padded + unpadded].copy_from_slice(row);
        }
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
    fn upload_format_follows_the_asset_colour_space() {
        assert_eq!(upload_format(true), wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(upload_format(false), wgpu::TextureFormat::Rgba8Unorm);
    }

    fn level(mip: u8, surface: u16) -> ow_scene::SceneTextureLevel {
        ow_scene::SceneTextureLevel {
            mip,
            surface,
            width: 4,
            height: 4,
            rgba: vec![0; 4 * 4 * 4],
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

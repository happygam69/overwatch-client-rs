//! Uploading a decoded texture, and the bind group the surface shader samples.
//!
//! One bind group per texture (group 1), a 1×1 white entry for draws with no
//! readable material, and a plain linear sampler: the install's textures ship
//! no sampler of their own that this project can read, and base colour at mip 0
//! is what the harness needs to answer "is this the hero's own texture".

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

fn upload_one(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    tex: &SceneTexture,
) -> wgpu::BindGroup {
    let (width, height) = (tex.width.max(1) as u32, tex.height.max(1) as u32);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("surface"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // The bytes are sRGB-encoded: a texture asset stored as BC1_UNORM_SRGB
        // or BC7_UNORM_SRGB is meant to be *sampled* as sRGB and linearised,
        // which is what this format does for us.
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // `write_texture` wants rows padded to 256 bytes.
    let unpadded = width as usize * 4;
    let padded = unpadded.div_ceil(256) * 256;
    let mut staging = Vec::with_capacity(padded * height as usize);
    for y in 0..height as usize {
        let at = y * unpadded;
        let row = tex
            .rgba
            .get(at..at + unpadded)
            .unwrap_or(&tex.rgba[0..unpadded.min(tex.rgba.len())]);
        staging.extend_from_slice(row);
        staging.resize((y + 1) * padded, 0);
    }
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &staging,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(padded as u32),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
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

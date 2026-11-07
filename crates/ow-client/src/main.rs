//! A first-person view of an Overwatch map, built from the install's own data.
//!
//! Nothing here is authored: the default launch resolves the practice range
//! through the map header (`STUMapHeader` -> variation scene map ->
//! `ow-tank::mapresolve` chunks), and the geometry comes from the models those
//! chunks place. What is missing is the client's own renderer -- textures are
//! used where the install carries a readable base colour, and the rest are
//! reported on a white placeholder.
//!
//! ```text
//! ow-client                         # the practice range, by header name
//! ow-client --practice-range        # the same, explicit
//! ow-client --list-maps             # every readable header and its modes
//! ow-client --shot out.png --width 1280 --height 720
//! ow-client --tour out --count 4
//! ow-client --chunks 0x0dd0000100016dc1   # explicit diagnostic, not a map
//! ow-client --list-heroes           # the practice mode's roster, with speeds
//! ow-client --hero tracer           # spawn with a hero from that roster
//! ```
//!
//! On a live launch with no `--hero`, the client asks for a pick before the map
//! loads (see `hero.rs`): the roster is the mode's read data, the applied
//! movement row is the hero's own, and the pick itself is a declared viewer
//! default because the real client's pick is a CEF page value.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use ow_scene as scene;
use ow_scene::Vertex;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

mod hero;
pub mod hud;
mod png;
mod surface;
mod walk;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_projection: [[f32; 4]; 4],
    camera: [f32; 4],
}

pub(crate) struct Camera {
    pub(crate) position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) speed: f32,
}

/// The client's `HorizontalFOV` setting: **103 deg**.
///
/// The setting is read by `GetHorizontalFov` (`module_v21_clean.bin`
/// `0x7ff7c6850fc0`). The first-person FOV conversion
/// `FUN_7ff7c5700f50` takes the half-angle in radians (`0.00872665` =
/// deg2rad/2, on-disk `.rdata` `0x7ff7c7cb3da8`), applies `tan`, multiplies by
/// the 9/16 aspect factor, applies `atan` and doubles; at 16:9 and 103 deg it
/// returns **70.53 deg vertical**. `ApplyFirstPersonCamera` sets that FOV on
/// the world camera each frame; the camera component's 70.0 deg
/// (`ConstructCameraComponent` `+0x48`) is the rounded 16:9 fallback.
const HORIZONTAL_FOV_DEG: f32 = 103.0;

/// World clip planes. `GetDefaultNearZ`/`GetDefaultFarZ` in
/// `module_v21_clean.bin` (`0x7ff7c686cbc0`/`0x7ff7c686cba0`) return
/// `0.1` / `16384.0` m.
pub(crate) const WORLD_NEAR: f32 = 0.1;
pub(crate) const WORLD_FAR: f32 = 16384.0;

/// First-person clip planes: `camera.FirstPersonNearZ`/`FarZ` are exactly one
/// tenth of the world pair (`0.01` / `1638.4` m).
pub(crate) const FP_NEAR: f32 = 0.01;
pub(crate) const FP_FAR: f32 = 1638.4;

/// Vertical FOV for a horizontal FOV of [`HORIZONTAL_FOV_DEG`] at `aspect`
/// (`width / height`).
fn vertical_fov(aspect: f32) -> f32 {
    let half = HORIZONTAL_FOV_DEG.to_radians() * 0.5;
    2.0 * (half.tan() / aspect.max(1e-3)).atan()
}

impl Camera {
    /// The view direction, in the mover's yaw convention.
    ///
    /// Movement and camera share `(sin yaw, cos yaw)` in (x, z); a mismatch
    /// makes W disagree with the view, which is what the earlier windowed
    /// run showed. The convention is pinned by a unit test.
    fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.position, self.forward(), Vec3::Y)
    }

    /// The camera's own up axis. The FP pass offset must be applied in this
    /// frame, not along world Y: the view model is camera-relative, so a
    /// world-Y drop swings the hands through the view as the camera pitches.
    fn up(&self) -> Vec3 {
        let q = glam::Quat::from_rotation_y(self.yaw) * glam::Quat::from_rotation_x(-self.pitch);
        q * Vec3::Y
    }
}

#[allow(dead_code)]
struct Renderer {
    window: Arc<Window>,
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    pub(crate) depth: wgpu::TextureView,
    pub(crate) uniform_buffer: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
    /// The first-person pass's own uniform buffer and bind group: the client
    /// gives objects rendered in first person their own near/far
    /// (`camera.FirstPersonNearZ`/`FarZ`, 0.01/1638.4 m).
    pub(crate) fp_uniform_buffer: wgpu::Buffer,
    pub(crate) fp_bind_group: wgpu::BindGroup,
    /// The first-person pass's camera framing, relative to the world eye.
    /// The client's FP camera transform is in the renderer pages absent from
    /// the earlier work, so these are **chosen** to frame the measured arm pose:
    /// the idle's hands sit ~0.38 m below the eye and ~0.09 m forward, so a
    /// level eye camera puts them under the screen. Lowering and backing the
    /// FP camera brings the hands and guns into the lower view without moving
    /// the world camera.
    // FIT-ANI-02: first-person camera framing: drop/back fitted
    pub(crate) fp_camera_drop: f32,
    pub(crate) fp_camera_back: f32,
    /// First-person pass vertical FOV in degrees, when the caller supplies
    /// one (`--fp-fov`). The client carries per-hero FP values in
    /// `STUFirstPersonComponent`; until its consumer is read, this stays an
    /// explicit experiment knob and the world FOV is the default.
    pub(crate) fp_fov: Option<f32>,
    /// Instances drawn in the first-person pass: the view model and its
    /// weapons. Every such placement has its own `Draw` with count 1.
    fp_instances: Vec<usize>,
    /// Each weapon instance and its frame **relative to the view model's
    /// root** (the FP skin's bind-pose bone matrix times the hardpoint's local
    /// matrix, the same product the scene placed it with). Re-composing it with
    /// the camera's first-person matrix each frame keeps the weapons in the
    /// hands as the player turns.
    weapon_rel: Vec<(usize, Mat4)>,
    /// One per `scene.textures`, indexed by `Draw::texture`.
    pub(crate) textures: Vec<wgpu::BindGroup>,
    /// The joint palette (group 2) and the buffer that backs it, kept alive.
    pub(crate) joints: (wgpu::Buffer, wgpu::BindGroup),
    /// The instance a view model sits at, when the scene placed one.
    pub(crate) view_model_instance: Option<usize>,
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) instance_buffer: wgpu::Buffer,
    pub(crate) draws: Vec<scene::Draw>,
    pub(crate) camera: Camera,
    /// The live mover, when the launch built one: the read velocity law over
    /// the range's own collision.
    walker: Option<walk::Walker>,
    /// The walk's input magnitude, metres per second.
    walk_speed: f32,
    /// Print the mover's state every frame (`--walk-log`).
    walk_log: bool,
    pub(crate) keys: HashMap<KeyCode, bool>,
    dragging: bool,
    last_mouse: Option<(f64, f64)>,
    /// The corner overlay: its texture, the bind group that samples it, the
    /// pipeline that draws it last, and the CPU grid baked into the texture.
    pub(crate) hud_texture: wgpu::Texture,
    pub(crate) hud_bind: wgpu::BindGroup,
    pub(crate) hud_globals: (wgpu::Buffer, wgpu::BindGroup),
    pub(crate) hud_pipeline: wgpu::RenderPipeline,
    hud_canvas: hud::HudCanvas,
}

#[allow(dead_code)]
impl Renderer {
    /// A renderer built from the real scene, whose first camera frame comes
    /// from a caller-supplied pose instead of the scene's drop-in eye. Passing
    /// the scene is what makes the window draw geometry at all; an empty scene
    /// renders as the clear colour and nothing else.
    pub(crate) fn new_posed(window: Arc<Window>, scene: &scene::Scene, pose: Camera) -> Renderer {
        let mut r = Renderer::new(window, scene, None, 5.5, false);
        r.camera = Camera {
            position: pose.position,
            yaw: pose.yaw,
            pitch: pose.pitch,
            speed: pose.speed,
        };
        r
    }

    fn new(
        window: Arc<Window>,
        scene: &scene::Scene,
        walker: Option<walk::Walker>,
        walk_speed: f32,
        walk_log: bool,
    ) -> Renderer {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("a graphics adapter");

        // R16_UNORM uploads need this optional feature. It is requested only
        // when the adapter advertises it; otherwise `surface::upload_format`
        // keeps the clamped 8-bit view instead of failing device creation.
        let mut features = wgpu::Features::empty();
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
        {
            features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ow-client"),
                required_features: features,
                // A whole map's geometry does not fit in the default 256 MB
                // buffer cap; the adapter's own limits do.
                required_limits: adapter.limits(),
                memory_hints: Default::default(),
            },
            None,
        ))
        .expect("a device");

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let fp_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("first-person uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("first-person uniforms"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: fp_uniform_buffer.as_entire_binding(),
            }],
        });
        let mut fp_instances = Vec::new();
        if let Some(i) = scene.view_model_instance {
            fp_instances.push(i);
        }
        fp_instances.extend(scene.weapon_attachments.iter().map(|(i, _, _)| *i));
        let weapon_rel: Vec<(usize, Mat4)> = {
            let bind = scene.skin.as_ref().map(|skin| skin.bind_world());
            scene
                .weapon_attachments
                .iter()
                .map(|(instance, bone, local)| {
                    let frame = match &bind {
                        Some(bind) => bind.get(*bone).copied().unwrap_or(Mat4::IDENTITY) * *local,
                        None => *local,
                    };
                    (*instance, frame)
                })
                .collect()
        };

        let surface_layout = surface::layout(&device);
        let textures = surface::upload_all(&device, &queue, &surface_layout, &scene.textures);
        let joint_layout = surface::joint_layout(&device);
        let joints = surface::joint_bind_group(&device, &queue, &joint_layout, &scene.joints);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[&bind_group_layout, &surface_layout, &joint_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[vertex_layout(), instance_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertices"),
            contents: bytemuck::cast_slice(&scene.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("indices"),
            contents: bytemuck::cast_slice(&scene.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("instances"),
            contents: bytemuck::cast_slice(&scene.instances),
            // COPY_DST so a first-person pass can move one instance per frame
            // (`Renderer::set_instance`) without rebuilding the buffer.
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        let depth = make_depth(&device, &config);

        // ---- the corner overlay ----------------------------------------
        // Text is baked into a 512x32 R8 grid by `hud::HudCanvas` and drawn as
        // one quad after the scene pass, alpha-blended, no depth ??? so it is
        // always on top and can never intersect the geometry.
        let hud_canvas = hud::HudCanvas::new();
        let (hud_w, hud_h) = hud_canvas.dims();
        let hud_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hud"),
            size: wgpu::Extent3d {
                width: hud_w,
                height: hud_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Blank first, so the first frame before any `set_hud` is defined.
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &hud_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            hud_canvas.data(),
            wgpu::ImageDataLayout {
                offset: 0,
                // The width is a multiple of 256 (64 columns x 8 px cells),
                // which is also the row-pitch rule `write_texture` keeps.
                bytes_per_row: Some(hud_w),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: hud_w,
                height: hud_h,
                depth_or_array_layers: 1,
            },
        );
        let hud_view = hud_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let hud_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hud"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        // Group 1 is the same texture+sampler shape the scene draws use, so the
        // scene's own bind group layout is reused for it.
        let hud_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hud"),
            layout: &surface_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hud_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&hud_sampler),
                },
            ],
        });
        let hud_globals_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hud globals"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let hud_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hud globals"),
            size: std::mem::size_of::<hud::Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &hud_globals_buf,
            0,
            bytemuck::bytes_of(&hud::globals(config.width as f32, config.height as f32)),
        );
        let hud_globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hud globals"),
            layout: &hud_globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: hud_globals_buf.as_entire_binding(),
            }],
        });
        let hud_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hud"),
            bind_group_layouts: &[&hud_globals_layout, &surface_layout],
            push_constant_ranges: &[],
        });
        let hud_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hud"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
        });
        let hud_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hud"),
            layout: Some(&hud_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &hud_shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &hud_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let camera = Camera {
            position: scene.eye,
            yaw: scene.yaw,
            pitch: scene.pitch,
            speed: 12.0,
        };

        Renderer {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            depth,
            uniform_buffer,
            bind_group,
            fp_uniform_buffer,
            fp_bind_group,
            fp_camera_drop: 0.0,
            fp_camera_back: 0.0,
            fp_fov: None,
            fp_instances,
            weapon_rel,
            textures,
            joints,
            view_model_instance: scene.view_model_instance,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            draws: scene.draws.clone(),
            camera,
            walker,
            walk_speed,
            walk_log,
            keys: HashMap::new(),
            dragging: false,
            last_mouse: None,
            hud_texture,
            hud_bind,
            hud_globals: (hud_globals_buf, hud_globals_bind),
            hud_pipeline,
            hud_canvas,
        }
    }

    /// Move one instance. Used by the first-person pass to park a view model on
    /// the camera every frame; the instance buffer is 64 bytes per placement, so
    /// this writes exactly one matrix.
    pub(crate) fn set_instance(&mut self, index: usize, matrix: Mat4) {
        let at = index as u64 * std::mem::size_of::<scene::Instance>() as u64;
        self.queue.write_buffer(
            &self.instance_buffer,
            at,
            bytemuck::bytes_of(&scene::Instance {
                model: matrix.to_cols_array_2d(),
            }),
        );
    }

    /// Whether every instance in this draw is a first-person placement (the
    /// view model or one of its weapons). Each has its own `Draw` with one
    /// instance, so the whole draw can move to the first-person pass.
    fn is_fp_draw(&self, draw: &scene::Draw) -> bool {
        let start = draw.instance_start as usize;
        let end = start + draw.instance_count as usize;
        start < end && (start..end).all(|i| self.fp_instances.contains(&i))
    }

    /// Replace the joint palette with a new pose.
    ///
    /// The palette is `[identity, bone 0, bone 1, ???]` ??? slot 0 is the identity so
    /// an unskinned vertex passes through unchanged (see `scene::Vertex::joints`).
    /// Shorter palettes pad with the identity, so a malformed mesh index cannot
    /// read another draw's bones.
    ///
    /// Called per frame by the animation pass; the buffer is already `COPY_DST`,
    /// so this is a straight write with no reallocation.
    pub(crate) fn set_joints(&mut self, palette: &[Mat4]) {
        let mut data: Vec<f32> = Vec::with_capacity(surface::MAX_JOINTS * 16);
        for m in palette.iter().take(surface::MAX_JOINTS) {
            data.extend_from_slice(&m.to_cols_array());
        }
        while data.len() < surface::MAX_JOINTS * 16 {
            data.extend_from_slice(&Mat4::IDENTITY.to_cols_array());
        }
        self.queue
            .write_buffer(&self.joints.0, 0, bytemuck::cast_slice(&data));
    }

    /// Re-bake the corner overlay's text into its texture.
    ///
    /// The client calls this every tenth frame with its HUD lines; the
    /// pass then reads whatever is baked, so an empty call simply hides the
    /// text. Lines beyond the overlay's rows and glyphs beyond its columns are
    /// clipped by [`hud::HudCanvas::lines`] ??? a report that cannot break the
    /// frame.
    pub(crate) fn set_hud(&mut self, lines: &[String]) {
        self.hud_canvas.lines(lines);
        let (w, h) = self.hud_canvas.dims();
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.hud_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.hud_canvas.data(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = make_depth(&self.device, &self.config);
        // The overlay quad keeps its pixel size; only the viewport it divides
        // into changes.
        self.queue.write_buffer(
            &self.hud_globals.0,
            0,
            bytemuck::bytes_of(&hud::globals(width as f32, height as f32)),
        );
    }

    fn update(&mut self, dt: f32) {
        if let Some(walker) = self.walker.as_mut() {
            let held = |code: KeyCode| self.keys.get(&code).copied().unwrap_or(false);
            // The input stage: the local wish in the character's own frame.
            // `+z` is forward and `+x` the character's left, which is the
            // handedness `R_y` and the rh camera share.
            let forward = held(KeyCode::KeyW) as i32 as f32 - held(KeyCode::KeyS) as i32 as f32;
            let strafe = held(KeyCode::KeyA) as i32 as f32 - held(KeyCode::KeyD) as i32 as f32;
            let wish = walk::wish_local(forward, strafe, self.walk_speed);
            // The command's button dword: bit 6 is the jump request and bit 7
            // the crouch request (`MoverUpdateFrame` `0x140cbb627` /
            // `0x140cbb63c`). The client's button source is the CEF input page;
            // the keyboard is this viewer's declared stand-in.
            walker.jump = held(KeyCode::Space);
            walker.crouch = held(KeyCode::ControlLeft) || held(KeyCode::ControlRight);
            let step = walker.step(wish, self.camera.yaw, dt);
            self.camera.position = Vec3::from_array(walker.eye());
            if self.walk_log {
                // The contact's normal and the shape that produced it are what
                // names an "invisible obstacle": shape `n` is
                // `RangeWorld::placed[n]`, and the last one is the level body.
                let contact = match (step.contact, step.contact_shape) {
                    (Some(n), Some(shape)) => {
                        format!(
                            "contact ({:+.2}, {:+.2}, {:+.2}) shape {shape}",
                            n[0], n[1], n[2]
                        )
                    }
                    _ => "contact none".to_string(),
                };
                println!(
                    "move pos ({:.3}, {:.3}, {:.3}) vel ({:.3}, {:.3}, {:.3}) contacts {} travelled {:.3} grounded {} {contact}",
                    step.pos[0],
                    step.pos[1],
                    step.pos[2],
                    step.velocity[0],
                    step.velocity[1],
                    step.velocity[2],
                    step.contacts,
                    step.travelled,
                    step.grounded
                );
            }
            self.place_first_person();
            return;
        }
        let forward = self.camera.forward();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let mut motion = Vec3::ZERO;
        let held = |code: KeyCode| self.keys.get(&code).copied().unwrap_or(false);
        if held(KeyCode::KeyW) {
            motion += forward;
        }
        if held(KeyCode::KeyS) {
            motion -= forward;
        }
        if held(KeyCode::KeyD) {
            motion += right;
        }
        if held(KeyCode::KeyA) {
            motion -= right;
        }
        if held(KeyCode::Space) {
            motion += Vec3::Y;
        }
        if held(KeyCode::ControlLeft) {
            motion -= Vec3::Y;
        }
        let boost = if held(KeyCode::ShiftLeft) { 4.0 } else { 1.0 };
        if motion.length_squared() > 0.0 {
            self.camera.position += motion.normalize() * self.camera.speed * boost * dt;
        }
        self.place_first_person();
    }

    /// Put the first-person view model and its weapons on the camera.
    ///
    /// The model's frame is [`scene::Scene::view_model_matrix`], the client's
    /// own first-person transform: the camera, moved down to the rig's feet,
    /// with nothing added. A weapon's frame is that matrix times the relative
    /// frame the scene placed it with. The **bind pose** is what is drawn — the
    /// client's per-frame first-person animation is not part of this pass, so
    /// the arms do not animate (`FIT-ANI-02`/`FIT-CAM-06`).
    fn place_first_person(&mut self) {
        let Some(view_model) = self.view_model_instance else {
            return;
        };
        let matrix = scene::Scene::view_model_matrix(
            self.camera.position,
            self.camera.yaw,
            self.camera.pitch,
        );
        self.set_instance(view_model, matrix);
        // Two entries at most; copying them keeps the borrow local to a frame.
        for (instance, relative) in self.weapon_rel.clone() {
            self.set_instance(instance, matrix * relative);
        }
    }

    fn render(&mut self) {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        // World projection: the client's 103-deg horizontal FOV converted to
        // vertical (70.53 deg at 16:9) and the read clip planes. The
        // first-person pair is the client's own (`camera.FirstPersonNearZ`/
        // `FarZ`, 0.01/1638.4 m).
        let fov = vertical_fov(aspect);
        let projection = Mat4::perspective_rh(fov, aspect, WORLD_NEAR, WORLD_FAR);
        let fp_fov = self.fp_fov.map(f32::to_radians).unwrap_or(fov);
        // FIT-CAM-03: FP pass: near/far read, the depth clear is fitted
        let fp_projection = Mat4::perspective_rh(fp_fov, aspect, FP_NEAR, FP_FAR);
        let uniforms = Uniforms {
            view_projection: (projection * self.camera.view()).to_cols_array_2d(),
            camera: [
                self.camera.position.x,
                self.camera.position.y,
                self.camera.position.z,
                1.0,
            ],
        };
        let fp_position = self.camera.position
            - self.camera.up() * self.fp_camera_drop
            - self.camera.forward() * self.fp_camera_back;
        let fp_view = Mat4::look_to_rh(fp_position, self.camera.forward(), Vec3::Y);
        let fp_uniforms = Uniforms {
            view_projection: (fp_projection * fp_view).to_cols_array_2d(),
            ..uniforms
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.queue
            .write_buffer(&self.fp_uniform_buffer, 0, bytemuck::bytes_of(&fp_uniforms));

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let mut fp_draws: Vec<usize> = Vec::new();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.55,
                            g: 0.62,
                            b: 0.72,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_bind_group(2, &self.joints.1, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            // One draw per distinct model; its placements come in as instances.
            for (index, draw) in self.draws.iter().enumerate() {
                // The view model and its weapons belong to the client's
                // first-person pass, not the world pass; they are drawn below
                // with the first-person projection.
                if self.is_fp_draw(draw) {
                    fp_draws.push(index);
                    continue;
                }
                // Group 1 is the draw's texture: white for a prop whose
                // material is not readable, the hero's own texture for the hero.
                if let Some(bind) = self.textures.get(draw.texture as usize) {
                    pass.set_bind_group(1, bind, &[]);
                }
                pass.draw_indexed(
                    draw.first_index..draw.first_index + draw.index_count,
                    draw.base_vertex,
                    draw.instance_start..draw.instance_start + draw.instance_count,
                );
            }
        }
        // The first-person pass. The client gives objects rendered in first
        // person their own projection (0.01/1638.4 m), which is why its arms do
        // not clip against the world camera's near plane. Clearing depth here
        // is *chosen*: the client's pass boundary is read, the clear is the
        // standard way to keep the view model out of wall geometry.
        if !fp_draws.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("first person"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.fp_bind_group, &[]);
            pass.set_bind_group(2, &self.joints.1, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            for &index in &fp_draws {
                let draw = &self.draws[index];
                if let Some(bind) = self.textures.get(draw.texture as usize) {
                    pass.set_bind_group(1, bind, &[]);
                }
                pass.draw_indexed(
                    draw.first_index..draw.first_index + draw.index_count,
                    draw.base_vertex,
                    draw.instance_start..draw.instance_start + draw.instance_count,
                );
            }
        }
        // The corner overlay: second pass, same view, load the scene's pixels,
        // no depth attachment, alpha-blended. One textured quad.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.hud_pipeline);
            pass.set_bind_group(0, &self.hud_globals.1, &[]);
            pass.set_bind_group(1, &self.hud_bind, &[]);
            pass.draw(0..4, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}

fn make_depth(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: config.width.max(1),
            height: config.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

struct App {
    scene: scene::Scene,
    renderer: Option<Renderer>,
    /// Frames drawn so far.
    frames: u64,
    /// Exit after this many frames (`--frames`), for a windowed smoke run.
    frame_limit: Option<u64>,
    /// The live mover, built in `main` and handed to the renderer on resume.
    walker: Option<walk::Walker>,
    walk_speed: f32,
    walk_log: bool,
    /// The corner overlay's lines: the picked hero and the walk speed.
    hero_lines: Vec<String>,
    /// The read D3D11 path, created by `--renderer d3d11`. D1 only clears and
    /// presents; the scene is loaded but not drawn on this path yet.
    d3d11: Option<ow_render::WindowSurface>,
    use_d3d11: bool,
    /// `--d3d11-frames N`: present N times and exit. A diagnostic for a
    /// headless run; the client itself has no such frame cap.
    d3d11_frames: Option<u32>,
    d3d11_presented: u32,
    window: Option<Arc<Window>>,
    last_frame: std::time::Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() || self.d3d11.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Overwatch map, read from the install")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));

        if self.use_d3d11 {
            let size = window.inner_size();
            let handle = window.window_handle().expect("a window handle");
            let RawWindowHandle::Win32(win32) = handle.as_raw() else {
                eprintln!("error: the D3D11 renderer needs a Win32 window handle");
                event_loop.exit();
                return;
            };
            let request = ow_render::SwapChainRequest::client_default(
                size.width.max(1),
                size.height.max(1),
                1,
            );
            match ow_render::WindowSurface::new(
                win32.hwnd.get(),
                ow_render::DeviceRequest::default(),
                request,
            ) {
                Ok(surface) => {
                    println!(
                        "renderer: d3d11 (stages {}, frame order {})",
                        ow_render::STAGES,
                        ow_render::frame::STAGE
                    );
                    println!("{}", surface.describe());
                    if let Err(error) = surface.log_views() {
                        eprintln!("error: D2 view description failed: {error}");
                        event_loop.exit();
                        return;
                    }
                    println!(
                        "D1-D3 draw no scene yet: the D3 step list runs and presents; no pass, clear value or draw call is read"
                    );
                    self.d3d11 = Some(surface);
                }
                Err(error) => {
                    eprintln!("error: the D3D11 device/swap chain could not be created: {error}");
                    if let Some(hr) = error.hresult() {
                        eprintln!("HRESULT: 0x{hr:08x}");
                    }
                    event_loop.exit();
                    return;
                }
            }
            self.window = Some(window);
            self.last_frame = std::time::Instant::now();
            return;
        }

        let mut renderer = Renderer::new(
            window.clone(),
            &self.scene,
            self.walker.take(),
            self.walk_speed,
            self.walk_log,
        );
        renderer.set_hud(&self.hero_lines);
        self.renderer = Some(renderer);
        self.window = Some(window);
        self.last_frame = std::time::Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // The D3D11 D1 path has no scene draw and no camera; it only keeps the
        // back buffer cleared and presented, and resizes the swap chain.
        if let Some(surface) = self.d3d11.as_mut() {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    if let Err(error) = surface.resize(size.width, size.height) {
                        eprintln!("error: swap-chain resize failed: {error}");
                        event_loop.exit();
                    }
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if event.state == ElementState::Pressed
                        && event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                    {
                        event_loop.exit();
                    }
                }
                WindowEvent::RedrawRequested => {
                    if let Err(error) =
                        ow_render::frame::run_frame(surface, self.d3d11_presented, true)
                    {
                        eprintln!("error: D3D11 frame failed: {error}");
                        event_loop.exit();
                        return;
                    }
                    self.d3d11_presented += 1;
                    if let Some(limit) = self.d3d11_frames {
                        if self.d3d11_presented >= limit {
                            event_loop.exit();
                            return;
                        }
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                _ => {}
            }
            return;
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(false) => {
                // Alt-tab must not leave a held key held: the OS delivers no
                // key-up for a press that happened before focus went away, so
                // the camera would keep drifting after the switch back. The
                // The free camera clears them the same way.
                renderer.keys.clear();
                renderer.dragging = false;
                renderer.last_mouse = None;
            }
            WindowEvent::Resized(size) => renderer.resize(size.width, size.height),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed && code == KeyCode::Escape {
                        event_loop.exit();
                    }
                    renderer
                        .keys
                        .insert(code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Right {
                    renderer.dragging = state == ElementState::Pressed;
                    if !renderer.dragging {
                        renderer.last_mouse = None;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if renderer.dragging {
                    if let Some((lx, ly)) = renderer.last_mouse {
                        let dx = (position.x - lx) as f32;
                        let dy = (position.y - ly) as f32;
                        renderer.camera.yaw -= dx * 0.004;
                        renderer.camera.pitch =
                            (renderer.camera.pitch - dy * 0.004).clamp(-1.5, 1.5);
                    }
                    renderer.last_mouse = Some((position.x, position.y));
                }
            }
            WindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                let dt = (now - self.last_frame).as_secs_f32().min(0.1);
                self.last_frame = now;
                renderer.update(dt);
                renderer.render();
                renderer.window.request_redraw();
                self.frames += 1;
                if let Some(limit) = self.frame_limit {
                    if self.frames >= limit {
                        if let Some(walker) = renderer.walker.as_ref() {
                            println!(
                                "windowed frames: {} at ({:.3}, {:.3}, {:.3})",
                                self.frames, walker.pos[0], walker.pos[1], walker.pos[2]
                            );
                        }
                        std::process::exit(0);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Render one frame offscreen and write it to a PNG, then exit.
///
/// A window is the right way to look at a map interactively, but it is the
/// wrong way to inspect twenty candidate viewpoints: each one means a restart
/// and a screenshot. Writing a file makes that a loop.
fn shot(path: &str, scene: &scene::Scene, width: u32, height: u32) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or("no graphics adapter")?;
    // Same optional feature gate as the windowed renderer: exact R16_UNORM
    // uploads when the adapter has the feature, the 8-bit fallback otherwise.
    let mut features = wgpu::Features::empty();
    if adapter
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
    {
        features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("ow-client shot"),
            required_features: features,
            required_limits: adapter.limits(),
            memory_hints: Default::default(),
        },
        None,
    ))
    .map_err(|e| e.to_string())?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let colour = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("colour"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let colour_view = colour.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scene"),
        source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
    });
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vertices"),
        contents: bytemuck::cast_slice(&scene.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("indices"),
        contents: bytemuck::cast_slice(&scene.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("instances"),
        contents: bytemuck::cast_slice(&scene.instances),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("uniforms"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("uniforms"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    let surface_layout = surface::layout(&device);
    let textures = surface::upload_all(&device, &queue, &surface_layout, &scene.textures);
    let joint_layout = surface::joint_layout(&device);
    let (_joint_buffer, joints) =
        surface::joint_bind_group(&device, &queue, &joint_layout, &scene.joints);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("scene"),
        bind_group_layouts: &[&bind_group_layout, &surface_layout, &joint_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[vertex_layout(), instance_layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });

    let aspect = width as f32 / height as f32;
    let projection = Mat4::perspective_rh(vertical_fov(aspect), aspect, WORLD_NEAR, WORLD_FAR);
    let camera = Camera {
        position: scene.eye,
        yaw: scene.yaw,
        pitch: scene.pitch,
        speed: 0.0,
    };
    let uniforms = Uniforms {
        view_projection: (projection * camera.view()).to_cols_array_2d(),
        camera: [scene.eye.x, scene.eye.y, scene.eye.z, 1.0],
    };
    queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

    // A texture-to-buffer copy requires each row to start on a 256-byte
    // boundary, so the readback is padded and the padding stripped afterwards.
    let unpadded = width * 4;
    let bytes_per_row = unpadded.div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shot"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.55,
                        g: 0.62,
                        b: 0.72,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_bind_group(2, &joints, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for draw in &scene.draws {
            if let Some(bind) = textures.get(draw.texture as usize) {
                pass.set_bind_group(1, bind, &[]);
            }
            pass.draw_indexed(
                draw.first_index..draw.first_index + draw.index_count,
                draw.base_vertex,
                draw.instance_start..draw.instance_start + draw.instance_count,
            );
        }
    }
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &colour,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range().to_vec();
    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height as usize {
        let start = row * bytes_per_row as usize;
        pixels.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    png::write_rgba(path, width, height, &pixels).map_err(|e| e.to_string())?;
    println!("wrote {path} ({width}x{height})");
    Ok(())
}

/// The vertex buffer layout: position and normal, one per vertex.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint8x4,
                offset: 32,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Unorm8x4,
                offset: 36,
                shader_location: 4,
            },
        ],
    }
}

/// The instance buffer layout: the placement, as four rows of a matrix.
fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<scene::Instance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 5,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 16,
                shader_location: 6,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 32,
                shader_location: 7,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 48,
                shader_location: 8,
            },
        ],
    }
}

/// The sweep world the walker uses: the map's own level collider, each shape
/// with its `MeshShapeFill` record and its world transform.
///
/// Only the `MAP_CHUNK_COLLISION` / additive bodies are here. The per-model
/// holder meshes are the models' own meshes: their read consumer in this image
/// is a query that writes nothing, and the per-model Domino body route hangs off
/// the collision component, whose attach no read path decides yet. Adding them
/// to the walk was measured (`--walk-test`, the practice range's spawn room):
/// the forward leg fell from `5.500` m to `5.189` m, the body drifted `0.461` m
/// off a straight line, the six-second tour climbed `0.78` m in a flat room, and
/// contacts doubled. See `docs/formats/character-controller.md`.
fn walk_world(range: &ow_tank::range_world::RangeWorld) -> Result<ow_sim::walk::World, String> {
    if range.level_shapes == 0 {
        return Err("the range carries no MAP_CHUNK_COLLISION body".into());
    }
    let mesh = &range.mesh;
    let level = range.level();
    let mut shapes = Vec::with_capacity(level.len());
    for placed in level {
        // The chunk stores the model's local-to-world quaternion; the sweep's
        // transform is the world-to-local one its leaf applies to the query.
        let rotation = placed.rotation;
        let conjugate = if rotation == [0.0; 4] {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            let len = (rotation[0] * rotation[0]
                + rotation[1] * rotation[1]
                + rotation[2] * rotation[2]
                + rotation[3] * rotation[3])
                .sqrt();
            if len > 0.0 {
                [
                    -rotation[0] / len,
                    -rotation[1] / len,
                    -rotation[2] / len,
                    rotation[3] / len,
                ]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            }
        };
        let record = mesh.record(placed.shape, placed.descriptor_scale());
        let (low, high) = placed.world_bounds();
        shapes.push(ow_sim::walk::Shape {
            record: record.to_vec(),
            xform: ow_sim::walk::shape_xform(placed.translation, conjugate),
            low,
            high,
        });
    }
    Ok(ow_sim::walk::World::new(
        mesh.base(),
        mesh.bytes().to_vec(),
        shapes,
    ))
}

/// Every surface a straight down-march meets in one column, with the world's
/// shape and triangle counts.
///
/// A single long sweep only reports the first hit; marching a small step and
/// restarting below each hit lists the whole stack, which is what says whether
/// a floor exists at a height the eye can see.
fn run_walk_column(walker: &walk::Walker, at: [f32; 2]) {
    let (low, high) = walker.world.extent();
    println!(
        "world: {} shape(s), {:.1} m of extent y, x {:.1}..{:.1}, z {:.1}..{:.1}",
        walker.world.shape_count(),
        high[1] - low[1],
        low[0],
        high[0],
        low[2],
        high[2]
    );
    let step = 0.25f32;
    let mut y = high[1] + 1.0;
    println!(
        "column at ({:.2}, {:.2}) from y {:.2} down",
        at[0], at[1], y
    );
    let mut found = 0;
    while y > low[1] - 1.0 {
        let from = [at[0], y, at[1]];
        let to = [at[0], y - step, at[1]];
        if let Some(hit) = walker.world.sweep(from, to, walk::RADIUS) {
            let contact = y - step * hit.fraction.clamp(0.0, 1.0);
            found += 1;
            if found <= 40 {
                println!(
                    "  surface {:2}: y {:.3} normal ({:+.2}, {:+.2}, {:+.2}) shape {}",
                    found,
                    contact - walk::RADIUS,
                    hit.normal[0],
                    hit.normal[1],
                    hit.normal[2],
                    hit.shape
                );
            }
            y = contact - 0.05;
        } else {
            y -= step;
        }
    }
    println!("{found} surface(s) crossed");
}

/// Step the mover with a scripted input and report the trace, so movement can
/// be checked from numbers instead of from a window.
///
/// The script: sixty frames of forward walk, sixty of standstill, sixty of
/// strafe-right. The assertions are the honest ones: the forward leg must
/// cover the speed times the time, the standstill must not drift, and the body
/// must stay on the floor and finite throughout.
fn run_walk_test(mut walker: walk::Walker, speed: f32) {
    let dt = 1.0 / 60.0;
    let start = walker.pos;
    println!(
        "walk test: start ({:.3}, {:.3}, {:.3}), speed {speed}, {} shape(s)",
        start[0],
        start[1],
        start[2],
        walker.world.shape_count()
    );
    let mut legs: Vec<(&str, f32, f32, f32, [f32; 3])> = Vec::new();
    let mut contacts = 0u32;
    let mut moved = 0.0f32;
    for (label, forward, strafe, frames) in [
        ("forward", 1.0f32, 0.0f32, 60),
        ("idle", 0.0, 0.0, 60),
        ("strafe", 0.0, -1.0, 60),
    ] {
        let before = walker.pos;
        let mut finites = true;
        for frame in 0..frames {
            let wish = walk::wish_local(forward, strafe, speed);
            let step = walker.step(wish, 0.0, dt);
            contacts += step.contacts;
            finites &= step.pos.iter().all(|v| v.is_finite());
            if frame % 20 == 0 {
                println!(
                    "  {label} {frame:02}: pos ({:.3}, {:.3}, {:.3}) grounded {} contacts {}",
                    step.pos[0], step.pos[1], step.pos[2], step.grounded, step.contacts
                );
            }
        }
        let dx = walker.pos[0] - before[0];
        let dz = walker.pos[2] - before[2];
        let travelled = (dx * dx + dz * dz).sqrt();
        moved += travelled;
        println!(
            "  {label}: travelled ({dx:.3}, {dz:.3}) = {travelled:.3} m, grounded {}, finite {finites}",
            walker.grounded
        );
        legs.push((label, dx, dz, travelled, walker.pos));
        assert!(finites, "{label} produced a non-finite position");
        assert!(
            walker.pos[1].is_finite() && walker.pos[1] > -1000.0,
            "{label} left the world"
        );
    }
    let (label, dx, dz, travelled, _) = legs[0];
    println!(
        "{label}: {travelled:.3} m in 1 s at {speed} m/s (x {dx:.3}, z {dz:.3}), {contacts} contact(s) over the run"
    );
    assert!(
        travelled > speed * 0.65,
        "{label} covered {travelled:.3} m in one second at {speed} m/s"
    );
    let (label, _, _, travelled, _) = legs[1];
    assert!(travelled < 0.05, "{label} drifted {travelled:.3} m");
    // A walk around the deck with the view turning, six seconds long: the
    // character must stay on the floor it started on, never drop through it.
    let deck = walker.pos;
    let mut lowest = deck[1];
    let mut tour_contacts = 0u32;
    let mut tour_shapes: std::collections::BTreeMap<usize, u32> = std::collections::BTreeMap::new();
    let mut tour_normals: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();
    for frame in 0..360 {
        let yaw = frame as f32 * 0.01;
        let step = walker.step(walk::wish_local(1.0, 0.0, speed), yaw, dt);
        lowest = lowest.min(step.pos[1]);
        tour_contacts += step.contacts;
        if let Some(shape) = step.contact_shape {
            *tour_shapes.entry(shape).or_default() += 1;
        }
        if let Some(n) = step.contact {
            let bucket = if n[1].abs() < ow_sim::walk::WALKABLE_SLOPE {
                format!("wall |ny|<{:.1}", ow_sim::walk::WALKABLE_SLOPE)
            } else if n[1] > 0.0 {
                "floor".to_string()
            } else {
                "ceiling".to_string()
            };
            *tour_normals.entry(bucket).or_default() += 1;
            if step.travelled < 0.5 && frame % 3 == 0 {
                println!(
                    "    blocked {frame:03}: shape {:?} normal ({:+.2}, {:+.2}, {:+.2}) travelled {:.3} pos ({:.2}, {:.2}, {:.2})",
                    step.contact_shape,
                    n[0],
                    n[1],
                    n[2],
                    step.travelled,
                    step.pos[0],
                    step.pos[1],
                    step.pos[2]
                );
            }
        }
    }
    println!(
        "  tour: from y {:.3} to y {:.3}, lowest {:.3} ({} contact(s))",
        deck[1], walker.pos[1], lowest, tour_contacts
    );
    println!("  tour: contact shapes {tour_shapes:?}");
    println!("  tour: contact classes {tour_normals:?}");
    assert!(
        lowest > deck[1] - 6.0,
        "the six-second walk fell {:.1} m below the deck it started on",
        deck[1] - lowest
    );
    walker.pos = deck;
    walker.fall = 0.0;

    // A drop from twenty metres up, no input: the fall must land on the first
    // floor below and stop there.
    walker.pos[1] += 20.0;
    walker.fall = 0.0;
    let mut landing = (0.0f32, 0u32);
    for frame in 0..240 {
        let step = walker.step([0.0; 3], 0.0, dt);
        if step.grounded && frame > 0 {
            landing = (step.pos[1], frame);
            println!(
                "  drop: landed at y {:.3} after {frame} frame(s)",
                step.pos[1]
            );
            break;
        }
        if frame % 30 == 0 {
            println!(
                "  drop {frame:03}: y {:.3} fall {:.2} grounded {}",
                step.pos[1], walker.fall, step.grounded
            );
        }
    }
    assert!(
        landing.1 > 0,
        "the drop never landed: ended at y {:.3}",
        walker.pos[1]
    );
    assert!(
        landing.0 > start[1] - 200.0,
        "the drop ended below the world at y {:.3}",
        landing.0
    );
    // Back on the spawn, for the legs' assertions below.
    walker.pos[1] = start[1];

    // The jump: take-off, arc and landing, from the read vertical law. The
    // take-off lane is the hero's `0xEC289987` (`6.0`) and gravity the row's
    // `0xC2C40969` (`1.75` -> `17.5` m/s^2), so the rise is a clean parabola:
    // apex `6^2 / (2 * 17.5) = 1.029` m at `0.343` s, and the fall starts from
    // there with the terminal-velocity drag. The drag is skipped while rising
    // because no hero on this install declares `body+0x618`.
    walker.pos = start;
    walker.fall = 0.0;
    walker.grounded = true;
    walker.jump = false;
    let _ = walker.step([0.0; 3], 0.0, dt);
    walker.jump = true;
    let mut apex = (f32::MIN, 0u32);
    let mut landed = (0u32, 0.0f32);
    for frame in 0..240 {
        let step = walker.step([0.0; 3], 0.0, dt);
        if frame == 0 {
            println!(
                "  jump: take-off lane {:.3} m/s, flags {:#010x}",
                walker.fall,
                walker.state_word()
            );
        }
        // A tap: the request is the take-off frame only.
        walker.jump = false;
        if step.pos[1] > apex.0 {
            apex = (step.pos[1], frame);
        }
        if frame > 2 && step.grounded {
            landed = (frame, step.pos[1]);
            break;
        }
    }
    let apex_height = apex.0 - start[1];
    let air_time = landed.0 as f32 * dt;
    // The semi-implicit step (`v += g dt` then `p += v dt`, the order
    // `0x140cb2b00` and the published vector give) lands one half-step lower
    // than the continuum: `v0^2/2g - v0 dt/2` and `2 v0/g - dt`. On the rise
    // the drag is skipped (no hero declares `body+0x618`), so both are exact
    // statements about the law, not tolerances on taste.
    let expect_apex = 6.0 * 6.0 / (2.0 * 17.5) - 6.0 * dt / 2.0;
    let expect_air = 2.0 * 6.0 / 17.5 - dt;
    println!(
        "  jump: apex {apex_height:.3} m (law {expect_apex:.3}) at frame {}; air {air_time:.3} s (law {expect_air:.3}), landed at y {:.3}",
        apex.1, landed.1
    );
    assert!(
        (apex_height - expect_apex).abs() < 0.01,
        "the apex is {apex_height:.3} m, the read law gives {expect_apex:.3} m"
    );
    assert!(
        (air_time - expect_air).abs() < 0.02,
        "the air time is {air_time:.3} s, the read law gives {expect_air:.3} s"
    );
    assert!(
        landed.1.is_finite() && (landed.1 - start[1]).abs() < 0.35,
        "the jump landed at y {:.3}, the floor is {:.3}",
        landed.1,
        start[1]
    );
    walker.fall = 0.0;

    let (label, dx, dz, travelled, _) = legs[2];
    assert!(
        travelled > speed * 0.4,
        "{label} covered only {travelled:.3} m (x {dx:.3}, z {dz:.3})"
    );

    // Walls: the room the mover spawns in is bounded. A *flat* wall is found by
    // requiring three parallel probes to agree on the normal, so a curved prop
    // sliding around its side is not mistaken for a wall. Walk straight at it
    // for two seconds: the body must stop short, must not climb it and must
    // never cross the wall's plane. This is the test the operator's report is
    // about ("i can go through walls").
    walker.pos = start;
    walker.fall = 0.0;
    walker.grounded = true;
    let chest = [start[0], start[1] + 0.9, start[2]];
    let mut walls: Vec<(f32, [f32; 3], [f32; 3], usize)> = Vec::new();
    for step in 0..48 {
        let angle = step as f32 * std::f32::consts::TAU / 48.0;
        let dir = [angle.cos(), 0.0, angle.sin()];
        let reach = 12.0f32;
        let to = [
            chest[0] + dir[0] * reach,
            chest[1],
            chest[2] + dir[2] * reach,
        ];
        let Some(hit) = walker.world.sweep(chest, to, walk::RADIUS) else {
            continue;
        };
        if hit.normal[1].abs() > 0.4 {
            continue;
        }
        let distance = hit.fraction * reach;
        if !(0.8..=10.0).contains(&distance) {
            continue;
        }
        // A flat wall: two probes half a metre to either side meet the same
        // plane with the same normal.
        let side = [-dir[2], 0.0, dir[0]];
        let mut flat = true;
        for s in [-0.5f32, 0.5] {
            let from = [chest[0] + side[0] * s, chest[1], chest[2] + side[2] * s];
            let to = [from[0] + dir[0] * reach, from[1], from[2] + dir[2] * reach];
            match walker.world.sweep(from, to, walk::RADIUS) {
                Some(near) => {
                    let d = (near.normal[0] - hit.normal[0]).abs()
                        + (near.normal[1] - hit.normal[1]).abs()
                        + (near.normal[2] - hit.normal[2]).abs();
                    if d > 0.05 {
                        flat = false;
                    }
                }
                None => flat = false,
            }
        }
        if !flat {
            continue;
        }
        // One wall per surface: the same shape with the same normal and the
        // same plane offset is the wall a neighbouring azimuth already found.
        let seen = walls.iter().any(|(_, _, n, shape)| {
            *shape == hit.shape
                && (n[0] - hit.normal[0]).abs() < 0.05
                && (n[1] - hit.normal[1]).abs() < 0.05
                && (n[2] - hit.normal[2]).abs() < 0.05
        });
        if !seen {
            walls.push((distance, dir, hit.normal, hit.shape));
        }
    }
    if walls.is_empty() {
        println!("  wall: no flat wall within 12 m of the spawn");
    }
    let mut crossed = 0;
    let mut tunnelled = 0u32;
    for (distance, dir, normal, shape) in &walls {
        walker.pos = start;
        walker.fall = 0.0;
        walker.grounded = true;
        let dir = *dir;
        let yaw = dir[0].atan2(dir[2]);
        let mut furthest = 0.0f32;
        let mut deepest = f32::INFINITY;
        let mut highest = start[1];
        let mut contact_frames = 0u32;
        for _ in 0..120 {
            let before = walker.pos;
            let step = walker.step(walk::wish_local(1.0, 0.0, speed), yaw, dt);
            // The centre may never cross a surface: a point sweep along the
            // frame's own path that meets a wall means the step tunnelled
            // through it, which is the operator's report ("i can go through
            // walls").
            let hit = walker.world.sweep(before, step.pos, 0.0);
            let crossed = hit.is_some_and(|h| h.normal[1].abs() < 0.6 && h.fraction < 0.999);
            if crossed {
                let h = hit.unwrap();
                println!(
                    "  tunnel! ({:.3},{:.3},{:.3}) -> ({:.3},{:.3},{:.3}) fraction {:.4} normal ({:+.2},{:+.2},{:+.2}) shape {}",
                    before[0], before[1], before[2],
                    step.pos[0], step.pos[1], step.pos[2],
                    h.fraction, h.normal[0], h.normal[1], h.normal[2], h.shape
                );
                tunnelled += 1;
            }
            let along = (step.pos[0] - start[0]) * dir[0] + (step.pos[2] - start[2]) * dir[2];
            furthest = furthest.max(along);
            // The signed gap to the wall's plane, along the normal the wall
            // reports (positive is the walker's side). A negative minimum means
            // the centre went past the wall.
            let plane = [chest[0] + dir[0] * distance, chest[2] + dir[2] * distance];
            let gap = (step.pos[0] - plane[0]) * normal[0] + (step.pos[2] - plane[1]) * normal[2];
            deepest = deepest.min(gap);
            highest = highest.max(step.pos[1]);
            if step.contacts > 0 {
                contact_frames += 1;
            }
        }
        println!(
            "  wall: shape {shape}, {distance:.2} m away, normal ({:+.2}, {:+.2}, {:+.2}); walked to {furthest:.2} m, closest approach to the plane {deepest:+.3} m, top of the climb {:+.2} m, {contact_frames} contact frame(s)",
            normal[0],
            normal[1],
            normal[2],
            highest - start[1]
        );
        if contact_frames == 0 {
            crossed += 1;
        }
        // 0.15 m is the most a walk may rise at a wall: the range's floors are
        // flat here, so anything more is the slide or the floor snap lifting the
        // body. Before the height rule this leg reported `+0.40 m` of climb and
        // ended `0.245 m` past the wall's plane.
        assert!(
            highest - start[1] < 0.15,
            "the walk climbed {:.2} m up a vertical wall",
            highest - start[1]
        );
        assert!(
            deepest > -0.05,
            "the walk crossed the wall's plane by {:.3} m",
            -deepest
        );
    }
    assert_eq!(
        crossed, 0,
        "{crossed} wall approach(es) never touched the wall"
    );
    assert_eq!(
        tunnelled, 0,
        "{tunnelled} frame(s) tunnelled through a surface"
    );
    println!("walk test: ok, {moved:.3} m total");
}

/// Audit the whole range for a walk that passes through geometry.
///
/// The grid: a vertical march every `STEP` metres over the level's own root box
/// collects every up-facing surface with a person's headroom. From each, eight
/// directions are walked for a second with the mover's own step, and every
/// frame's centre path is swept with a point query. A point sweep that meets a
/// near-vertical surface before the frame's endpoint means the step moved the
/// centre across solid geometry: a tunnel, and the thing the operator reports
/// as walking through walls.
fn run_wall_audit(mut walker: walk::Walker, speed: f32) {
    const STEP: f32 = 12.0;
    const RADIUS: f32 = walk::RADIUS;
    const DT: f32 = 1.0 / 60.0;
    let (low, high) = walker.world.extent();
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut x = low[0] + STEP * 0.5;
    while x < high[0] {
        let mut z = low[2] + STEP * 0.5;
        while z < high[2] {
            let mut y = high[1] - 0.5;
            while y > low[1] {
                let from = [x, y, z];
                let to = [x, y - 0.5, z];
                match walker.world.sweep(from, to, RADIUS) {
                    Some(hit) => {
                        let at = y - 0.5 * hit.fraction.clamp(0.0, 1.0);
                        y = at - 0.05;
                        if hit.normal[1] <= 0.9 {
                            continue;
                        }
                        // Headroom: a 1.9 m person must fit.
                        let head = walker
                            .world
                            .sweep([x, at + 0.1, z], [x, at + 1.9, z], RADIUS);
                        if head.is_some() {
                            continue;
                        }
                        points.push([x, at, z]);
                        if points.len() >= 4000 {
                            break;
                        }
                    }
                    None => y -= 0.5,
                }
            }
            z += STEP;
        }
        x += STEP;
    }
    println!(
        "wall audit: {} standable point(s) over the level",
        points.len()
    );
    let mut legs = 0u32;
    let mut tunnels = 0u32;
    // A point sweep over a slope is a false positive: the chord between two
    // positions on a rising surface lies *below* it, so the sweep meets the
    // surface the walker is climbing. Count the near-vertical ones (|up| < 0.3,
    // steeper than 72 degrees) separately; that is the wall count.
    let mut wall_tunnels = 0u32;
    let mut contacts = 0u32;
    for point in &points {
        for direction in 0..8 {
            let angle = direction as f32 * std::f32::consts::TAU / 8.0;
            let dir = [angle.cos(), 0.0, angle.sin()];
            let yaw = dir[0].atan2(dir[2]);
            // A leg that starts inside a prop is not a walk: require the
            // body to have room in front of it before the first frame.
            let ahead = [point[0] + dir[0] * 0.6, point[1], point[2] + dir[2] * 0.6];
            if let Some(hit) = walker.world.sweep(*point, ahead, RADIUS) {
                if hit.fraction < 0.6 {
                    continue;
                }
            }
            walker.pos = *point;
            walker.fall = 0.0;
            walker.grounded = true;
            legs += 1;
            for _ in 0..60 {
                let before = walker.pos;
                let step = walker.step(walk::wish_local(1.0, 0.0, speed), yaw, DT);
                contacts += step.contacts;
                let crossed = walker
                    .world
                    .sweep(before, step.pos, 0.0)
                    .is_some_and(|h| h.normal[1].abs() < 0.6 && h.fraction < 0.999);
                if crossed {
                    let h = walker.world.sweep(before, step.pos, 0.0).unwrap();
                    tunnels += 1;
                    if h.normal[1].abs() < 0.3 {
                        wall_tunnels += 1;
                    }
                    if tunnels <= 20 {
                        println!(
                            "  tunnel at ({:.2},{:.2},{:.2}) -> ({:.2},{:.2},{:.2}) fraction {:.4} normal ({:+.2},{:+.2},{:+.2}) shape {}",
                            before[0], before[1], before[2],
                            step.pos[0], step.pos[1], step.pos[2],
                            h.fraction, h.normal[0], h.normal[1], h.normal[2], h.shape
                        );
                    }
                }
            }
        }
    }
    println!(
        "wall audit: {legs} leg(s), {contacts} contact(s), {tunnels} tunnelled frame(s) ({wall_tunnels} on a near-vertical wall)"
    );
}

/// Assemble the range's collision world and put a walker on the scene's spawn.
///
/// The spawn is the explicit `--spawn x y z` when given, otherwise the map's
/// spawn-room trigger volume (the practice range's hero-select room), otherwise
/// the scene's own viewpoint. The map's own spawn selectors are unread, so the
/// volume is the viewer's declared default; it is map data, not a guess at the
/// client's rule.
fn build_walker(scene: &scene::Scene, spawn: Option<[f32; 3]>) -> Result<walk::Walker, String> {
    use ow_tank::classreg::ClassRegistry;
    use ow_tank::keytable::Sources;
    use ow_tank::range_world::RangeWorld;
    use ow_tank::{GameData, ManifestSelector};
    let root = ow_casc::install::detect_root().ok_or("no install root")?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = GameData::open(&root, &ManifestSelector::default(), &sources)
        .map_err(|e| format!("the install does not open: {e}"))?;
    let classreg = ClassRegistry::load_from_sources(&sources);
    if classreg.is_empty() {
        return Err("no STU class checkout found".into());
    }
    let range = RangeWorld::build(&data, &sources, classreg.inner())?;
    let spawn = match (spawn, range.spawn_room) {
        (Some(at), _) => at,
        (None, Some(room)) => {
            println!(
                "spawn: map spawn-room volume centre ({:.3}, {:.3}, {:.3}), half extents ({:.3}, {:.3}, {:.3}), floor {:.3}",
                room.centre[0],
                room.centre[1],
                room.centre[2],
                room.extents[0],
                room.extents[1],
                room.extents[2],
                room.floor()
            );
            room.floor_probe()
        }
        (None, None) => scene.eye.to_array(),
    };
    let world = walk_world(&range)?;
    Ok(walk::Walker::new(world, spawn))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--renderer d3d11` selects the client's production backend. The default
    // stays the wgpu viewer, which is still viewer-only (`FIT-PRE-02`).
    let use_d3d11 = match args
        .iter()
        .position(|a| a == "--renderer")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
    {
        Some("d3d11") => true,
        Some(other) => {
            eprintln!("error: unknown --renderer {other} (expected d3d11; the default is wgpu)");
            std::process::exit(2);
        }
        None => false,
    };
    let d3d11_frames = args
        .iter()
        .position(|a| a == "--d3d11-frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u32>().ok());

    // ---- the hero-select stage -------------------------------------------
    // The mode's roster is read before the map, so a pick can pre-place the
    // hero's own first-person model in the scene. `--list-heroes` prints the
    // roster and exits; `--hero` names a pick; `--no-hero` skips the prompt.
    // The prompt itself is offered only to a live walk with a terminal.
    let free_cam = args.iter().any(|a| a == "--free-cam") || args.iter().any(|a| a == "--chunks");
    let hero_spec = match args.iter().position(|a| a == "--hero") {
        Some(at) => match args.get(at + 1) {
            Some(value) if !value.starts_with("--") => Some(value.clone()),
            _ => {
                eprintln!("error: --hero needs a hero name or an asset index");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let list_heroes = args.iter().any(|a| a == "--list-heroes");
    let no_hero = args.iter().any(|a| a == "--no-hero");
    let want_prompt = !no_hero
        && hero_spec.is_none()
        && !use_d3d11
        && !free_cam
        && hero::interactive_launch(&args);
    let mut roster: Option<hero::ModeRoster> = None;
    let mut picked: Option<usize> = None;
    if list_heroes || hero_spec.is_some() || want_prompt {
        match hero::practice_roster() {
            Ok(mode) => {
                if list_heroes {
                    println!(
                        "hero select: {} (mode 0x{:016x}) - {}",
                        mode.mode_name,
                        mode.mode,
                        mode.rule_summary()
                    );
                    println!(
                        "  {} playable hero(es); {} NPC hero asset(s) the mode names but a picker does not offer",
                        mode.heroes.len(),
                        mode.npc_heroes
                    );
                    for line in hero::table(&mode) {
                        println!("  {line}");
                    }
                    println!("  --hero <name|index> picks one");
                    std::process::exit(0);
                }
                match hero::choose(&mode, hero_spec.as_deref(), want_prompt) {
                    Ok(Some(entry)) => {
                        picked = mode
                            .heroes
                            .iter()
                            .position(|h| h.hero.guid == entry.hero.guid);
                        roster = Some(mode);
                    }
                    Ok(None) => roster = Some(mode),
                    Err(e) => {
                        if hero_spec.is_some() {
                            eprintln!("error: {e}");
                            std::process::exit(2);
                        }
                        eprintln!("note: hero select: {e}; no hero selected");
                    }
                }
            }
            Err(e) => {
                if list_heroes || hero_spec.is_some() {
                    eprintln!("error: hero select is unavailable: {e}");
                    std::process::exit(2);
                }
                eprintln!("note: hero select is unavailable ({e}); no hero");
            }
        }
    }
    let pick: Option<hero::HeroEntry> = match (roster.as_ref(), picked) {
        (Some(mode), Some(index)) => Some(mode.heroes[index].clone()),
        _ => None,
    };

    // `--hero` belongs to this binary: strip it (and its value) from the scene
    // arguments, then add the canonical form when a hero was picked. The
    // scene's own `--hero` takes the asset index and places the view model.
    let mut scene_args = args.clone();
    if let Some(at) = scene_args.iter().position(|a| a == "--hero") {
        let end = (at + 2).min(scene_args.len());
        scene_args.drain(at..end);
    }
    if let Some(pick) = &pick {
        scene_args.push("--hero".into());
        scene_args.push(pick.hero.index.to_string());
        scene_args.push("--view-model".into());
        scene_args.push("--first-person".into());
    }
    let scene = match scene::build(&scene_args) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "{} triangles from {} placements across {} models",
        scene.triangle_count(),
        scene.placements,
        scene.models_loaded
    );

    // A tour: many viewpoints from one load, so a map can be looked over
    // without paying the load cost per view.
    if let Some(at) = args.iter().position(|a| a == "--tour") {
        let dir = args.get(at + 1).cloned().unwrap_or_else(|| "tour".into());
        let count: usize = args
            .iter()
            .position(|a| a == "--count")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(16);
        std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .unwrap_or(());
        for (i, eye) in scene.tour_points(count).into_iter().enumerate() {
            println!(
                "tour view {i:02}: eye ({:.2}, {:.2}, {:.2})",
                eye.x, eye.y, eye.z
            );
            for (j, yaw) in [0.0f32, std::f32::consts::FRAC_PI_2, std::f32::consts::PI]
                .iter()
                .enumerate()
            {
                let mut view = scene::Scene::view_from(eye, *yaw);
                view.vertices = scene.vertices.clone();
                view.indices = scene.indices.clone();
                view.instances = scene.instances.clone();
                view.draws = scene.draws.clone();
                view.textures = scene.textures.clone();
                let path = format!("{dir}/view_{i:02}_{j}.png");
                if let Err(e) = shot(&path, &view, 960, 540) {
                    eprintln!("{path}: {e}");
                }
            }
        }
        std::process::exit(0);
    }

    if let Some(at) = args.iter().position(|a| a == "--shot") {
        let path = args
            .get(at + 1)
            .cloned()
            .unwrap_or_else(|| "shot.png".into());
        let width = args
            .iter()
            .position(|a| a == "--width")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1280u32);
        let height = args
            .iter()
            .position(|a| a == "--height")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(720u32);
        // A row of the readback buffer is `width * 4` bytes padded to a
        // multiple of 256; the padding arithmetic and the `width * height`
        // buffer size both overflow `u32` for a careless `--width`, which
        // panics in a debug build and allocates nonsense in a release one.
        const MAX_SHOT_SIDE: u32 = 16_384;
        if width == 0 || height == 0 || width > MAX_SHOT_SIDE || height > MAX_SHOT_SIDE {
            eprintln!("error: --width/--height must be 1..={MAX_SHOT_SIDE} (got {width}x{height})");
            std::process::exit(1);
        }
        match shot(&path, &scene, width, height) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }

    // The live mover: the range's own model meshes as the collision world, then
    // the read per-frame velocity law. An explicit `--chunks` launch is a
    // diagnostic view with no coherent range to stand in, so it keeps the free
    // camera; `--free-cam` forces the same.
    //
    // The input's magnitude is the input source's (item 1b). `--speed` wins;
    // otherwise a picked hero's own forward base speed (`0x19452F15`) is used;
    // otherwise the install-wide common value 5.5 (`docs/formats/character-controller.md`).
    let walk_speed = args
        .iter()
        .position(|a| a == "--speed")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok())
        .or_else(|| pick.as_ref().and_then(|hero| hero.forward_speed()))
        .unwrap_or(walk::SPEED);
    let walk_log = args.iter().any(|a| a == "--walk-log");
    let spawn_override = args.iter().position(|a| a == "--spawn").map(|i| {
        let at = |n: usize| {
            args.get(i + n)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        [at(1), at(2), at(3)]
    });
    let walker = if free_cam {
        None
    } else {
        match build_walker(&scene, spawn_override) {
            Ok(mut walker) => {
                if let Some(terminal) = pick.as_ref().and_then(|hero| hero.terminal_fall()) {
                    walker.terminal_fall = terminal;
                }
                if let Some(rise) = pick.as_ref().and_then(|hero| hero.rise_terminal()) {
                    walker.rise_terminal = rise;
                }
                if let Some(gravity) = pick.as_ref().and_then(|hero| hero.gravity()) {
                    walker.gravity = gravity;
                }
                if let Some(lane) = pick.as_ref().and_then(|hero| hero.takeoff_lane()) {
                    walker.takeoff_lane = lane;
                }
                if let Some(reach) = pick.as_ref().and_then(|hero| hero.step_reach()) {
                    walker.floor_reach = reach;
                }
                println!(
                    "walk: {} level-collision shape(s), standing at ({:.2}, {:.2}, {:.2})",
                    walker.world.shape_count(),
                    walker.pos[0],
                    walker.pos[1],
                    walker.pos[2]
                );
                Some(walker)
            }
            Err(e) => {
                eprintln!("note: the collision world is not built ({e}); the free camera is used");
                None
            }
        }
    };

    if let Some(hero) = &pick {
        println!(
            "hero: {} (asset index {}, 0x{:016x}){}",
            hero.hero.label(),
            hero.hero.index,
            hero.hero.guid,
            match roster.as_ref() {
                Some(mode) => format!(
                    "; selectable in {} (mode 0x{:016x}, {})",
                    mode.mode_name,
                    mode.mode,
                    mode.rule_summary()
                ),
                None => String::new(),
            }
        );
        if let Some(row) = hero.row {
            let v = |value: Option<f32>| match value {
                Some(value) => format!("{value:.3}"),
                None => "absent".to_string(),
            };
            println!(
                "move: forward {}, side {}, back {}, crouch {}, terminal fall {} (the hero's own mover row; the input stage is the viewer's, so the wish is {walk_speed:.3} m/s)",
                v(row.forward),
                v(row.side),
                v(row.back),
                v(row.crouch),
                v(row.fall_terminal)
            );
        }
    }
    let scene_for_column = args
        .iter()
        .any(|a| a == "--render-column")
        .then(|| scene::build(&scene_args));
    if let Some(i) = args.iter().position(|a| a == "--walk-column") {
        let mut at = [0.0f32, 0.0];
        if let (Some(x), Some(z)) = (args.get(i + 1), args.get(i + 2)) {
            at = [x.parse().unwrap_or(0.0), z.parse().unwrap_or(0.0)];
        }
        match walker {
            Some(walker) => {
                run_walk_column(&walker, at);
                if let Some(Ok(scene)) = scene_for_column.as_ref() {
                    render_column(scene, at);
                }
            }
            None => {
                eprintln!("error: --walk-column needs the collision world");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--wall-audit") {
        match walker {
            Some(walker) => run_wall_audit(walker, walk_speed),
            None => {
                eprintln!("error: --wall-audit needs the collision world");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--walk-test") {
        match walker {
            Some(walker) => run_walk_test(walker, walk_speed),
            None => {
                eprintln!("error: --walk-test needs the collision world");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    let hero_lines = match &pick {
        Some(hero) => vec![
            format!("HERO {}", hero.hero.label()),
            format!("SPEED {walk_speed:.2} M/S"),
        ],
        None => vec![
            "HERO NONE".to_string(),
            format!("SPEED {walk_speed:.2} M/S"),
        ],
    };

    if walker.is_some() {
        println!("ow-client: movement live. WASD walks, right-drag looks, escape quits");
    } else {
        println!("ow-client: inspector mode; gameplay movement/combat are not implemented yet");
        println!("right-drag to look, WASD to move, shift to hurry, escape to quit");
    }

    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        scene,
        renderer: None,
        frames: 0,
        frame_limit: args
            .iter()
            .position(|a| a == "--frames")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<u64>().ok()),
        walker,
        walk_speed,
        walk_log,
        hero_lines,
        d3d11: None,
        use_d3d11,
        d3d11_frames,
        d3d11_presented: 0,
        window: None,
        last_frame: std::time::Instant::now(),
    };
    event_loop.run_app(&mut app).expect("the app runs");
}

#[cfg(test)]
mod camera_tests {
    use super::{vertical_fov, Camera};
    use ow_scene as scene;

    /// The viewer uses the client's 103 deg `HorizontalFOV`; converting back
    /// must give 103 deg at every aspect ratio. At 16:9 the vertical FOV is
    /// 70.53 deg, and the first-person FOV conversion returns the same value.
    #[test]
    fn vertical_fov_reproduces_103_horizontal() {
        for aspect in [16.0 / 9.0, 4.0 / 3.0, 21.0 / 9.0, 1.0] {
            let v = vertical_fov(aspect);
            let h = 2.0 * ((v * 0.5).tan() * aspect).atan();
            assert!(
                (h.to_degrees() - 103.0).abs() < 1e-3,
                "aspect={aspect} horizontal={}",
                h.to_degrees()
            );
        }
    }

    /// The per-frame first-person frame is the camera's own: `view *
    /// view_model_matrix(eye, yaw, pitch)` is the same matrix at every pose —
    /// a half turn about `y` (the rig's forward is `+Z`, the view's is `-Z`)
    /// and the rig's feet, `EYE_HEIGHT` below the eye. This is what
    /// `Renderer::place_first_person` relies on when it rewrites the view
    /// model's instance matrix from the live camera instead of leaving it at
    /// the spawn pose.
    #[test]
    fn the_view_model_frame_is_the_camera_frame_at_every_pose() {
        let poses = [
            (glam::Vec3::new(0.0, 2.0, 0.0), 0.0f32, 0.0f32),
            (glam::Vec3::new(71.17, -1.9, 53.11), 1.2, -0.4),
            (glam::Vec3::new(-40.0, 12.5, 8.0), -2.7, 0.9),
        ];
        for (position, yaw, pitch) in poses {
            let camera = Camera {
                position,
                yaw,
                pitch,
                speed: 0.0,
            };
            let relative = camera.view() * scene::Scene::view_model_matrix(position, yaw, pitch);
            // Column-major: `m[column][row]`. `rotY(pi)` is `diag(-1, 1, -1)`.
            let m = relative.to_cols_array_2d();
            let want = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
            for col in 0..3 {
                for row in 0..3 {
                    assert!(
                        (m[col][row] - want[col][row]).abs() < 1e-4,
                        "basis column {col} row {row} = {} at ({position:?}, {yaw}, {pitch})",
                        m[col][row]
                    );
                }
            }
            let feet = glam::Vec3::new(m[3][0], m[3][1], m[3][2]);
            let expected = glam::Vec3::new(0.0, -scene::EYE_HEIGHT, 0.0);
            assert!(
                (feet - expected).length() < 1e-4,
                "translation {feet:?} at ({position:?}, {yaw}, {pitch})"
            );
        }
    }
}

#[cfg(test)]
mod walk_tests {
    use super::walk;

    /// The input stage's vector is in the character's own frame: `+z` forward,
    /// `+x` the character's left, unit length before the speed is applied. The
    /// state transform is a rotation about `y`, which maps `+z` to the camera's
    /// forward `(sin yaw, cos yaw)` and `+x` to `-(that right)`, so pressing `a`
    /// (a positive strafe in `wish_local`'s argument) walks to the left.
    #[test]
    fn the_wish_is_local_and_unit_length() {
        let forward = walk::wish_local(1.0, 0.0, 5.5);
        assert_eq!(forward, [0.0, 0.0, 5.5]);
        let strafe = walk::wish_local(0.0, 1.0, 5.5);
        assert_eq!(strafe, [5.5, 0.0, 0.0]);
        let diagonal = walk::wish_local(1.0, 1.0, 5.5);
        let length = (diagonal[0] * diagonal[0] + diagonal[2] * diagonal[2]).sqrt();
        assert!(
            (length - 5.5).abs() < 1e-5,
            "a diagonal wish is {length}, not the speed"
        );
        assert_eq!(walk::wish_local(0.0, 0.0, 5.5), [0.0; 3]);
    }

    /// A quarter turn to the right (`yaw = 0`) must send the local forward
    /// wish along the camera's forward, and the local left wish to the
    /// camera's screen-left.
    #[test]
    fn the_body_frame_matches_the_camera_frame() {
        let yaw = std::f32::consts::FRAC_PI_2;
        let wish = walk::wish_local(1.0, 0.0, 2.0);
        let matrix = glam::Mat4::from_rotation_y(yaw).to_cols_array();
        // What `MoverStateVector` does: the matrix's first three columns.
        let world = [
            matrix[0] * wish[0] + matrix[4] * wish[1] + matrix[8] * wish[2],
            matrix[1] * wish[0] + matrix[5] * wish[1] + matrix[9] * wish[2],
            matrix[2] * wish[0] + matrix[6] * wish[1] + matrix[10] * wish[2],
        ];
        // The camera's own forward at that yaw is (sin, cos) in (x, z).
        assert!((world[0] - 2.0).abs() < 1e-6, "x {}", world[0]);
        assert!(world[2].abs() < 1e-6, "z {}", world[2]);
        let left = walk::wish_local(0.0, 1.0, 2.0);
        let world = [
            matrix[0] * left[0] + matrix[8] * left[2],
            matrix[1] * left[0] + matrix[9] * left[2],
            matrix[2] * left[0] + matrix[10] * left[2],
        ];
        // Screen-left at yaw = pi/2 is -z (the camera looks along +x).
        assert!((world[2] + 2.0).abs() < 1e-6, "z {}", world[2]);
    }
}

/// Every surface of the **render** geometry a vertical line meets, for
/// comparison with [`run_walk_column`]'s collision surfaces.
///
/// The scene's triangles are instance-local; each draw's instances carry the
/// world matrix. A vertical line at `(x, z)` meets a triangle where the
/// triangle's plane solves to a `y` on the right side of all three edges.
fn render_column(scene: &scene::Scene, at: [f32; 2]) {
    let mut heights: Vec<f32> = Vec::new();
    let mut owners: Vec<(f32, u64)> = Vec::new();
    for draw in &scene.draws {
        let first = draw.first_index as usize;
        let count = draw.index_count as usize;
        for instance in
            draw.instance_start as usize..(draw.instance_start + draw.instance_count) as usize
        {
            let m = glam::Mat4::from_cols_array_2d(&scene.instances[instance].model);
            let mut tris = count / 3;
            let mut t = 0;
            while tris > 0 {
                let mut p = [[0.0f32; 3]; 3];
                for (k, point) in p.iter_mut().enumerate() {
                    let index = scene.indices[first + t * 3 + k] as i32 + draw.base_vertex;
                    let v = &scene.vertices[index as usize];
                    *point = m
                        .transform_point3(glam::Vec3::from_array(v.position))
                        .to_array();
                }
                if let Some(y) = vertical_hit(p, at) {
                    heights.push(y);
                    owners.push((y, draw.model));
                }
                t += 1;
                tris -= 1;
            }
        }
    }
    heights.sort_by(f32::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    println!(
        "render column at ({:.2}, {:.2}): {} surface(s)",
        at[0],
        at[1],
        heights.len()
    );
    for y in heights.iter().take(60) {
        let model = owners
            .iter()
            .find(|(h, _)| (h - y).abs() < 1e-3)
            .map(|(_, m)| *m)
            .unwrap_or(0);
        println!("  render surface: y {y:.3} model {model:#x}");
    }
}

/// Where a vertical line through `(x, z)` meets a triangle, if it does.
fn vertical_hit(p: [[f32; 3]; 3], at: [f32; 2]) -> Option<f32> {
    // Barycentric coordinates in the x-z plane, then the plane's y there.
    let (x0, z0) = (p[0][0], p[0][2]);
    let (x1, z1) = (p[1][0], p[1][2]);
    let (x2, z2) = (p[2][0], p[2][2]);
    let det = (z1 - z2) * (x0 - x2) + (x2 - x1) * (z0 - z2);
    if det.abs() < 1e-12 {
        return None;
    }
    let a = ((z1 - z2) * (at[0] - x2) + (x2 - x1) * (at[1] - z2)) / det;
    let b = ((z2 - z0) * (at[0] - x2) + (x0 - x2) * (at[1] - z2)) / det;
    let c = 1.0 - a - b;
    if a < 0.0 || b < 0.0 || c < 0.0 {
        return None;
    }
    Some(a * p[0][1] + b * p[1][1] + c * p[2][1])
}

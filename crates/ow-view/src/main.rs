//! A first-person view of an Overwatch map, built from the install's own data.
//!
//! Nothing here is authored: the geometry comes from map chunk assets, and the
//! models they place come from model assets. What is missing is materials --
//! textures are the next layer -- so surfaces are shaded from their normals
//! instead, which is enough to see whether the geometry is right.
//!
//! ```text
//! ow-view --chunks 0x0dd0000100016dc1
//! ow-view --chunks 0x...,0x... --at 10,2,30
//! ow-view --list [--count 40]
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use ow_scene as scene;
use ow_scene::Vertex;
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

pub mod anim_tree;
pub mod hud;
pub mod play;
mod png;
mod surface;

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

/// The client's default `HorizontalFOV`: **103??**. The settings registration
/// at v2b `0x7ff7c51dd6a8` writes `0x42ce0000` to `0x7ff7c8f83db8`; the var is
/// the same `HorizontalFOV` setting at `0x7ff7c87f2f9c`. The client fixes the
/// horizontal FOV and derives the vertical one from the aspect.
const HORIZONTAL_FOV_DEG: f32 = 103.0;

/// Vertical FOV for a horizontal FOV of [`HORIZONTAL_FOV_DEG`] at `aspect`
/// (`width / height`). At 16:9 this is ???70??, which is why the old hardcoded
/// 70?? looked right there and drifted on other window shapes.
fn vertical_fov(aspect: f32) -> f32 {
    let half = HORIZONTAL_FOV_DEG.to_radians() * 0.5;
    2.0 * (half.tan() / aspect.max(1e-3)).atan()
}

impl Camera {
    /// The view direction, in the **mover's** yaw convention.
    ///
    /// The movement axes are **measured** from live traces (tier 1 in
    /// `rewrite-plan.md` ?? M6, checked to 0.006??) and are
    /// `forward = (sin yaw, cos yaw)` in (x, z) (`ow-sim`'s controller, and
    /// its direction test). The camera must use the same convention or the view
    /// and the W direction disagree by 90?? ??? which is what a mismatch here
    /// looked like in the window: turning the mouse rotated the view but W kept
    /// moving along the old axis.
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
    /// every capture, so these are **fitted** to frame the measured arm pose:
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

impl Renderer {
    /// A renderer built from the real scene, whose first camera frame comes
    /// from a pose instead of the scene's drop-in eye (the play window owns the
    /// pose). Passing the scene is what makes the play window draw geometry at
    /// all ??? an empty scene renders as the clear colour and nothing else.
    pub(crate) fn new_posed(window: Arc<Window>, scene: &scene::Scene, pose: Camera) -> Renderer {
        let mut r = Renderer::new(window, scene);
        r.camera = Camera {
            position: pose.position,
            yaw: pose.yaw,
            pitch: pose.pitch,
            speed: pose.speed,
        };
        r
    }

    fn new(window: Arc<Window>, scene: &scene::Scene) -> Renderer {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("a graphics adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ow-view"),
                required_features: wgpu::Features::empty(),
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
            textures,
            joints,
            view_model_instance: scene.view_model_instance,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            draws: scene.draws.clone(),
            camera,
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
    /// The play window calls this every tenth frame with its HUD lines; the
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
    }

    fn render(&mut self) {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        // FIT-CAM-04: world camera near/far: stand-in (only the FP pair is read)
        // World projection. The client's **world** near/far are not read, so
        // this remains the viewer's stand-in; the first-person pair below is
        // the client's own (`camera.FirstPersonNearZ`/`FarZ`, v2b
        // `0x7ff7c56b24e0` region, defaults 0.01/1638.4 m). Both use the
        // client's 103?? horizontal FOV, converted to the vertical FOV the
        // projection matrix takes.
        let fov = vertical_fov(aspect);
        let projection = Mat4::perspective_rh(fov, aspect, 0.01, 8000.0);
        let fp_fov = self.fp_fov.map(f32::to_radians).unwrap_or(fov);
        // FIT-CAM-03: FP pass: near/far read, the depth clear is fitted
        let fp_projection = Mat4::perspective_rh(fp_fov, aspect, 0.01, 1638.4);
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
        // is *fitted*: the client's pass boundary is read, the clear is the
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
    last_frame: std::time::Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Overwatch map, read from the install")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        self.renderer = Some(Renderer::new(window, &self.scene));
        self.last_frame = std::time::Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("ow-view shot"),
            required_features: wgpu::Features::empty(),
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
    let projection = Mat4::perspective_rh(vertical_fov(aspect), aspect, 0.05, 8000.0);
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let scene = match scene::build(&args) {
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
        match shot(&path, &scene, width, height) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }

    // ---- Play mode ---------------------------------------------------------
    if args.iter().any(|a| a == "--play-th") {
        if let Err(e) = play::headless(&args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    // --play: the interactive window. It carries its own flag through to App.

    if args.iter().any(|a| a == "--play") {
        println!(
            "--play: WASD + Space to move, mouse-locked look, left click to fire, escape to quit \
             (sandbox course by default; name --chunks/--chunks-from/--cluster to play on the map)"
        );
        if let Err(e) = play::windowed(&args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    println!("right-drag to look, WASD to move, shift to hurry, escape to quit");

    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        scene,
        renderer: None,
        last_frame: std::time::Instant::now(),
    };
    event_loop.run_app(&mut app).expect("the app runs");
}

#[cfg(test)]
mod camera_tests {
    use super::vertical_fov;

    /// The vertical FOV is derived from the client's 103?? horizontal value,
    /// so converting back must give 103?? at every aspect ratio.
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
}

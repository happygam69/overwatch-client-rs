//! The native D3D11 backend: device, swap chain and swap-chain views
//! (stages D1 and D2).
//!
//! This is the production path of the client's own renderer, read from the
//! complete `Overwatch.exe` image. The operations follow the client's
//! `pm_dx11` and `pm_dxgi` code one for one:
//!
//! 1. enumerate DXGI adapters from index 0 (`pm_dxgi::Instance::InitGpus`,
//!    `0x14262f0e9`);
//! 2. create the device through `D3D11CreateDevice` with the client's
//!    adapter, `D3D_DRIVER_TYPE_UNKNOWN`, no software module, the read flag
//!    expression, the `11_1`-first feature-level list and SDK version 7
//!    (`pm_dx11::Instance::CreateDevice`, `0x142666980`, through the vendor
//!    wrapper at `0x14267d270`);
//! 3. create the swap chain with `CreateSwapChainForHwnd` and a
//!    `DXGI_SWAP_CHAIN_DESC1` built from the engine view's request
//!    (`pm_dxgi::Instance::InitSwapChain`, `0x14262f9d0`);
//! 4. create the back-buffer texture and its views
//!    (`pm_dx11::Device::CreateSwapChainResources`, `0x14266a060`):
//!    `GetBuffer(0)`, the RTV, then the SRV when usage bit 0 is set and the
//!    UAV when usage bit 1 is set, each with the client's explicit
//!    descriptor.
//!
//! Resize follows `pm_dxgi::Instance::UpdateSwapChain` (`0x1426320f0`) with
//! the two callbacks it invokes: release the old set in the order texture →
//! RTV → SRV → UAV (`FUN_14266bd90`), `ResizeBuffers`, then
//! `CreateSwapChainResources` again (`FUN_14265ff80`).
//!
//! **What D1/D2 prove:** the creation values and the view descriptors are
//! read and pinned by unit tests, and this module issues the same calls with
//! them. **What they do not:** the device, swap-chain and view calls are not
//! yet compared against a D3D11 capture oracle (RenderDoc/PIX is not
//! installed). See `docs/formats/renderer.md`.

use std::fmt;

use windows::core::ComInterface;
use windows::Win32::Foundation::{FALSE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_SRV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_CREATE_DEVICE_FLAG,
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RENDER_TARGET_VIEW_DESC_0, D3D11_RTV_DIMENSION_TEXTURE2D,
    D3D11_SDK_VERSION, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0,
    D3D11_TEX2D_RTV, D3D11_TEX2D_SRV, D3D11_TEX2D_UAV, D3D11_TEXTURE2D_DESC,
    D3D11_UAV_DIMENSION_TEXTURE2D, D3D11_UNORDERED_ACCESS_VIEW_DESC,
    D3D11_UNORDERED_ACCESS_VIEW_DESC_0,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE, DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIFactory2, IDXGIOutput,
    IDXGISwapChain1, DXGI_SCALING, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT, DXGI_USAGE,
};

use crate::request::{
    present_flags, DeviceRequest, RenderTargetViewDesc, ShaderResourceViewDesc, SwapChainRequest,
    UnorderedAccessViewDesc,
};

/// Which stage of the client's renderer this module implements.
pub const STAGE: &str = "D1";
/// The swap-chain resource stage, implemented in this module.
pub const VIEW_STAGE: &str = "D2";
/// The stages this module now carries.
pub const STAGES: &str = "D1+D2";

/// The client's D3D11 adapter enumeration starts at index 0. The preference
/// order the client applies after enumeration is **open**, so D1 takes the
/// first adapter that can create a device.
pub const ADAPTER_INDEX: u32 = 0;

/// The colour D1 clears to. No client clear constant is read, and the colour
/// clear opcode is not identified, so this is a visible D1 placeholder rather
/// than a client colour.
pub const D1_CLEAR_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

#[derive(Debug)]
pub enum Error {
    /// A Win32/D3D call returned a failing HRESULT.
    Windows {
        call: &'static str,
        source: windows::core::Error,
    },
    /// `EnumAdapters1(0)` returned no adapter.
    NoAdapter,
    /// A create call succeeded but left a null COM pointer.
    MissingObject(&'static str),
    /// The engine view's format byte points outside the 64-entry table the
    /// client reads. The client has no range check; the value past the table
    /// is not a read mapping, so no view is created.
    UnreadViewFormat(u8),
}

impl Error {
    fn windows(call: &'static str, source: windows::core::Error) -> Self {
        Self::Windows { call, source }
    }

    /// The HRESULT as a signed 32-bit value, when this is a Win32 failure.
    pub fn hresult(&self) -> Option<i32> {
        match self {
            Self::Windows { source, .. } => Some(source.code().0),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Windows { call, source } => {
                write!(f, "{call} failed: 0x{:08x} ({source})", source.code().0)
            }
            Self::NoAdapter => write!(f, "EnumAdapters1({ADAPTER_INDEX}) returned no adapter"),
            Self::MissingObject(what) => write!(f, "{what} was null after a successful call"),
            Self::UnreadViewFormat(byte) => write!(
                f,
                "the engine format byte 0x{byte:02x} is outside the read view-format table"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Windows { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// `D3D_FEATURE_LEVEL`'s read name, for a log line.
pub fn feature_level_name(level: u32) -> &'static str {
    match level {
        x if x == crate::request::FEATURE_LEVEL_11_1 => "D3D_FEATURE_LEVEL_11_1",
        x if x == crate::request::FEATURE_LEVEL_11_0 => "D3D_FEATURE_LEVEL_11_0",
        x if x == crate::request::FEATURE_LEVEL_10_1 => "D3D_FEATURE_LEVEL_10_1",
        x if x == crate::request::FEATURE_LEVEL_10_0 => "D3D_FEATURE_LEVEL_10_0",
        x if x == crate::request::FEATURE_LEVEL_9_3 => "D3D_FEATURE_LEVEL_9_3",
        x if x == crate::request::FEATURE_LEVEL_9_2 => "D3D_FEATURE_LEVEL_9_2",
        x if x == crate::request::FEATURE_LEVEL_9_1 => "D3D_FEATURE_LEVEL_9_1",
        _ => "unknown D3D_FEATURE_LEVEL",
    }
}

/// The device created by `D3D11CreateDevice`.
pub struct Device {
    /// The one factory used for adapter enumeration and the swap chain, as the
    /// client's `pm_dxgi::Instance` does.
    factory: IDXGIFactory1,
    adapter_name: String,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// The level D3D11 actually selected.
    feature_level: u32,
    /// The flag word passed in, from the client's expression.
    creation_flags: u32,
}

impl Device {
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn feature_level(&self) -> u32 {
        self.feature_level
    }

    pub fn creation_flags(&self) -> u32 {
        self.creation_flags
    }

    pub fn raw(&self) -> &ID3D11Device {
        &self.device
    }

    pub fn context(&self) -> &ID3D11DeviceContext {
        &self.context
    }
}

/// One back buffer and the views that address it.
///
/// The field order is the client's release order in `FUN_14266bd90` and is
/// also Rust's drop order: texture, render-target view, shader-resource view,
/// unordered-access view.
struct SwapResources {
    _buffer: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    srv: Option<ID3D11ShaderResourceView>,
    uav: Option<ID3D11UnorderedAccessView>,
    /// `ID3D11Texture2D::GetDesc`'s reported size, as the client records it.
    width: u32,
    height: u32,
}

/// A window's D3D11 device and swap chain.
pub struct WindowSurface {
    device: Device,
    swap_chain: IDXGISwapChain1,
    resources: Option<SwapResources>,
    request: SwapChainRequest,
    /// The descriptor DXGI reports after creation; a mismatched request is
    /// visible here rather than silently assumed.
    actual: DXGI_SWAP_CHAIN_DESC1,
}

/// The D2 result, as plain values: the back buffer's reported size, the
/// engine and resolved formats, and which of the three views exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapChainViewDescription {
    pub width: u32,
    pub height: u32,
    pub engine_format: u8,
    pub view_format: u32,
    pub usage_bits: u8,
    pub render_target: bool,
    pub shader_resource: bool,
    pub unordered_access: bool,
    /// The explicit RTV descriptor the client passes.
    pub rtv: RenderTargetViewDesc,
    /// The explicit SRV descriptor, when usage bit 0 is set.
    pub srv: Option<ShaderResourceViewDesc>,
    /// The explicit UAV descriptor, when usage bit 1 is set.
    pub uav: Option<UnorderedAccessViewDesc>,
}

impl fmt::Display for SwapChainViewDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "back buffer: {}x{}, engine format 0x{:02x}, view format {}, usage bits 0x{:x}; \
             RTV format={} dim={} mip={}",
            self.width,
            self.height,
            self.engine_format,
            self.view_format,
            self.usage_bits,
            self.rtv.format,
            self.rtv.view_dimension,
            self.rtv.mip_slice
        )?;
        match self.srv {
            Some(srv) => write!(
                f,
                "; SRV format={} dim={} mip={} levels={}",
                srv.format, srv.view_dimension, srv.most_detailed_mip, srv.mip_levels
            )?,
            None => write!(f, "; SRV absent")?,
        }
        match self.uav {
            Some(uav) => write!(
                f,
                "; UAV format={} dim={} mip={}",
                uav.format, uav.view_dimension, uav.mip_slice
            )?,
            None => write!(f, "; UAV absent")?,
        }
        Ok(())
    }
}

/// A plain snapshot of the created values, for a log or a test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceDescription {
    pub adapter_name: String,
    pub feature_level: u32,
    pub feature_level_name: &'static str,
    pub creation_flags: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub buffer_count: u32,
    pub buffer_usage: u32,
    pub swap_effect: u32,
    pub scaling: u32,
    pub alpha_mode: u32,
    pub flags: u32,
    pub sync_interval: u32,
}

impl fmt::Display for SurfaceDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "adapter: {}", self.adapter_name)?;
        writeln!(
            f,
            "feature level: 0x{:04x} ({})",
            self.feature_level, self.feature_level_name
        )?;
        writeln!(f, "creation flags: 0x{:02x}", self.creation_flags)?;
        writeln!(
            f,
            "swap chain: {}x{} format={} buffers={} usage=0x{:x} swap_effect={} scaling={} alpha={} flags=0x{:x}",
            self.width,
            self.height,
            self.format,
            self.buffer_count,
            self.buffer_usage,
            self.swap_effect,
            self.scaling,
            self.alpha_mode,
            self.flags
        )?;
        write!(f, "sync interval: {}", self.sync_interval)
    }
}

impl WindowSurface {
    /// Create the D3D11 device and a swap chain for `hwnd`.
    ///
    /// `request` is the swap-chain request; use [`SwapChainRequest::client_default`]
    /// for the client's default view. `device_request` carries the three read
    /// flag inputs and the optional feature-level override.
    pub fn new(
        hwnd: isize,
        device_request: DeviceRequest,
        request: SwapChainRequest,
    ) -> Result<Self, Error> {
        let device = create_device(&device_request)?;
        let swap_chain = create_swap_chain(&device, HWND(hwnd), &request)?;
        let actual = unsafe { swap_chain_desc(&swap_chain)? };
        let resources = create_swap_resources(&device.device, &swap_chain, &request)?;
        Ok(Self {
            device,
            swap_chain,
            resources: Some(resources),
            request,
            actual,
        })
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn request(&self) -> &SwapChainRequest {
        &self.request
    }

    pub fn describe(&self) -> SurfaceDescription {
        SurfaceDescription {
            adapter_name: self.device.adapter_name.clone(),
            feature_level: self.device.feature_level,
            feature_level_name: feature_level_name(self.device.feature_level),
            creation_flags: self.device.creation_flags,
            width: self.actual.Width,
            height: self.actual.Height,
            format: self.actual.Format.0 as u32,
            buffer_count: self.actual.BufferCount,
            buffer_usage: self.actual.BufferUsage.0,
            swap_effect: self.actual.SwapEffect.0 as u32,
            scaling: self.actual.Scaling.0 as u32,
            alpha_mode: self.actual.AlphaMode.0 as u32,
            flags: self.actual.Flags,
            sync_interval: self.request.sync_interval,
        }
    }

    /// The D2 view result: size, formats and which views exist.
    pub fn view_description(&self) -> Result<SwapChainViewDescription, Error> {
        let Some(resources) = self.resources.as_ref() else {
            return Err(Error::MissingObject("swap-chain render target"));
        };
        let view = self.request.view_request();
        let view_format = view
            .view_format()
            .ok_or(Error::UnreadViewFormat(view.engine_format))?;
        Ok(SwapChainViewDescription {
            width: resources.width,
            height: resources.height,
            engine_format: view.engine_format,
            view_format,
            usage_bits: view.usage_bits,
            render_target: true,
            shader_resource: resources.srv.is_some(),
            unordered_access: resources.uav.is_some(),
            rtv: view
                .rtv_desc()
                .ok_or(Error::UnreadViewFormat(view.engine_format))?,
            srv: view.srv_desc(),
            uav: view.uav_desc(),
        })
    }

    /// Print the D2 views as one `renderer: D2:` line.
    pub fn log_views(&self) -> Result<(), Error> {
        println!("renderer: D2: {}", self.view_description()?);
        Ok(())
    }

    /// The present flag word, from the client's four read conditions.
    ///
    /// The instance tearing input (`instance+0xb0`), present `+0x48` and
    /// present `+0x49` are config/runtime fields whose shipped values are
    /// **open**; this path does not take the tearing branch, so the three
    /// inputs are false and the flag is zero.
    pub fn present_flags(&self) -> u32 {
        present_flags(false, self.request.sync_interval, false, false)
    }

    /// `pm_dxgi::Instance::Present` (`0x142630790`) through
    /// `IDXGISwapChain1::Present`: the request's sync interval and the flag
    /// expression above.
    pub fn present(&self) -> Result<(), Error> {
        unsafe {
            self.swap_chain
                .Present(self.request.sync_interval, self.present_flags())
                .ok()
                .map_err(|e| Error::windows("IDXGISwapChain1::Present", e))?;
        }
        Ok(())
    }

    /// Clear the back buffer with [`D1_CLEAR_COLOUR`] only.
    ///
    /// This is a declared D1 placeholder, not a client value: no clear colour
    /// constant is read and the colour-clear opcode is open.
    pub fn clear_placeholder(&self) -> Result<(), Error> {
        let Some(resources) = self.resources.as_ref() else {
            return Err(Error::MissingObject("swap-chain render target"));
        };
        unsafe {
            self.device
                .context
                .ClearRenderTargetView(&resources.rtv, &D1_CLEAR_COLOUR);
        }
        Ok(())
    }

    /// Clear with the D1 placeholder colour, then present. Retained for the
    /// D1 smoke run; the D3 driver calls the steps separately.
    pub fn clear_and_present(&mut self) -> Result<(), Error> {
        self.clear_placeholder()?;
        self.present()
    }

    /// Resize the back buffers with the client's count and format
    /// (`UpdateSwapChain`, `0x1426320f0`), in the read view order:
    /// release texture → RTV → SRV → UAV (`FUN_14266bd90`), `ResizeBuffers`,
    /// then `CreateSwapChainResources` again (`FUN_14265ff80`).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.request.width && height == self.request.height {
            return Ok(());
        }
        // The read sequence, pinned by `SwapChainViewRequest::resize_steps`:
        // release texture → RTV → SRV → UAV, resize, GetBuffer → RTV →
        // optional SRV/UAV. The release is the `SwapResources` drop below.
        // DXGI requires every reference to the old back buffers to be released
        // before ResizeBuffers. `SwapResources`' field order is the client's
        // release order, so dropping the set here is that step.
        self.resources = None;
        unsafe {
            self.swap_chain
                .ResizeBuffers(
                    self.request.buffer_count,
                    width,
                    height,
                    DXGI_FORMAT(self.request.format as i32),
                    0,
                )
                .map_err(|e| Error::windows("IDXGISwapChain1::ResizeBuffers", e))?;
        }
        self.resources = Some(create_swap_resources(
            &self.device.device,
            &self.swap_chain,
            &self.request,
        )?);
        self.request.width = width;
        self.request.height = height;
        self.actual = unsafe { swap_chain_desc(&self.swap_chain)? };
        Ok(())
    }
}

/// `pm_dx11::Instance::CreateDevice` (`0x142666980`) with the read values.
fn create_device(request: &DeviceRequest) -> Result<Device, Error> {
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1().map_err(|e| Error::windows("CreateDXGIFactory1", e))? };
    let adapter: IDXGIAdapter1 = unsafe {
        factory
            .EnumAdapters1(ADAPTER_INDEX)
            .map_err(|_| Error::NoAdapter)?
    };
    let adapter_name = unsafe {
        let mut desc = windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_DESC1::default();
        adapter
            .GetDesc1(&mut desc)
            .map_err(|e| Error::windows("IDXGIAdapter1::GetDesc1", e))?;
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        String::from_utf16_lossy(&desc.Description[..end])
    };
    let adapter0: IDXGIAdapter = adapter
        .cast()
        .map_err(|e| Error::windows("IDXGIAdapter1::cast", e))?;

    // The client requests 11_1 alone first. A failure is the only trigger for
    // the six-level retry; an explicit override skips the retry entirely.
    let primary = request.primary_feature_levels();
    let flags = request.creation_flags();
    let (device, context, level) = match try_create_device(&adapter0, flags, &primary) {
        Ok(created) => created,
        Err(first) => match request.fallback_feature_levels() {
            Some(fallback) => try_create_device(&adapter0, flags, &fallback)
                .map_err(|e| e.or_context("D3D11CreateDevice (fallback)"))?,
            None => return Err(first.or_context("D3D11CreateDevice (override)")),
        },
    };

    Ok(Device {
        factory,
        adapter_name,
        device,
        context,
        feature_level: level.0 as u32,
        creation_flags: flags,
    })
}

fn try_create_device(
    adapter: &IDXGIAdapter,
    flags: u32,
    levels: &[u32],
) -> Result<(ID3D11Device, ID3D11DeviceContext, D3D_FEATURE_LEVEL), Error> {
    let levels: Vec<D3D_FEATURE_LEVEL> = levels
        .iter()
        .map(|level| D3D_FEATURE_LEVEL(*level as i32))
        .collect();
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut level = D3D_FEATURE_LEVEL(0);
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE(0),
            D3D11_CREATE_DEVICE_FLAG(flags),
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            Some(&mut context),
        )
        .map_err(|e| Error::windows("D3D11CreateDevice", e))?;
    }
    Ok((
        device.ok_or(Error::MissingObject("ID3D11Device"))?,
        context.ok_or(Error::MissingObject("ID3D11DeviceContext"))?,
        level,
    ))
}

fn create_swap_chain(
    device: &Device,
    hwnd: HWND,
    request: &SwapChainRequest,
) -> Result<IDXGISwapChain1, Error> {
    let factory2: IDXGIFactory2 = device
        .factory
        .cast()
        .map_err(|e| Error::windows("IDXGIFactory1::cast(IDXGIFactory2)", e))?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: request.width,
        Height: request.height,
        Format: DXGI_FORMAT(request.format as i32),
        Stereo: FALSE,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE(request.buffer_usage),
        BufferCount: request.buffer_count,
        Scaling: DXGI_SCALING(request.scaling as i32),
        SwapEffect: DXGI_SWAP_EFFECT(request.swap_effect as i32),
        AlphaMode: DXGI_ALPHA_MODE(request.alpha_mode as i32),
        Flags: request.flags,
    };
    unsafe {
        factory2
            .CreateSwapChainForHwnd(&device.device, hwnd, &desc, None, None::<&IDXGIOutput>)
            .map_err(|e| Error::windows("CreateSwapChainForHwnd", e))
    }
}

unsafe fn swap_chain_desc(swap_chain: &IDXGISwapChain1) -> Result<DXGI_SWAP_CHAIN_DESC1, Error> {
    let mut desc = DXGI_SWAP_CHAIN_DESC1::default();
    swap_chain
        .GetDesc1(&mut desc)
        .map_err(|e| Error::windows("IDXGISwapChain1::GetDesc1", e))?;
    Ok(desc)
}

/// `pm_dx11::Device::CreateSwapChainResources` (`0x14266a060`).
///
/// Read order, exactly: `GetBuffer(0, ID3D11Texture2D)`; resolve the view
/// format from the engine format byte; `CreateRenderTargetView` with the
/// explicit texture-2D descriptor; `ID3D11Texture2D::GetDesc` for the
/// reported size; then `CreateShaderResourceView` when usage bit 0 is set and
/// `CreateUnorderedAccessView` when usage bit 1 is set, each with its own
/// explicit descriptor.
fn create_swap_resources(
    device: &ID3D11Device,
    swap_chain: &IDXGISwapChain1,
    request: &SwapChainRequest,
) -> Result<SwapResources, Error> {
    let view = request.view_request();
    let rtv_desc = view
        .rtv_desc()
        .ok_or(Error::UnreadViewFormat(view.engine_format))?;
    let buffer: ID3D11Texture2D = unsafe {
        swap_chain
            .GetBuffer(0)
            .map_err(|e| Error::windows("IDXGISwapChain1::GetBuffer", e))?
    };
    let rtv = create_render_target_view(device, &buffer, &rtv_desc)?;
    let (width, height) = unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        buffer.GetDesc(&mut desc);
        (desc.Width, desc.Height)
    };
    let srv = match view.srv_desc() {
        Some(desc) => Some(create_shader_resource_view(device, &buffer, &desc)?),
        None => None,
    };
    let uav = match view.uav_desc() {
        Some(desc) => Some(create_unordered_access_view(device, &buffer, &desc)?),
        None => None,
    };
    Ok(SwapResources {
        _buffer: buffer,
        rtv,
        srv,
        uav,
        width,
        height,
    })
}

fn create_render_target_view(
    device: &ID3D11Device,
    buffer: &ID3D11Texture2D,
    desc: &RenderTargetViewDesc,
) -> Result<ID3D11RenderTargetView, Error> {
    let native = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT(desc.format as i32),
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_RTV {
                MipSlice: desc.mip_slice,
            },
        },
    };
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    unsafe {
        device
            .CreateRenderTargetView(buffer, Some(&native), Some(&mut rtv))
            .map_err(|e| Error::windows("ID3D11Device::CreateRenderTargetView", e))?;
    }
    rtv.ok_or(Error::MissingObject("ID3D11RenderTargetView"))
}

fn create_shader_resource_view(
    device: &ID3D11Device,
    buffer: &ID3D11Texture2D,
    desc: &ShaderResourceViewDesc,
) -> Result<ID3D11ShaderResourceView, Error> {
    let native = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT(desc.format as i32),
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: desc.most_detailed_mip,
                MipLevels: desc.mip_levels,
            },
        },
    };
    let mut srv: Option<ID3D11ShaderResourceView> = None;
    unsafe {
        device
            .CreateShaderResourceView(buffer, Some(&native), Some(&mut srv))
            .map_err(|e| Error::windows("ID3D11Device::CreateShaderResourceView", e))?;
    }
    srv.ok_or(Error::MissingObject("ID3D11ShaderResourceView"))
}

fn create_unordered_access_view(
    device: &ID3D11Device,
    buffer: &ID3D11Texture2D,
    desc: &UnorderedAccessViewDesc,
) -> Result<ID3D11UnorderedAccessView, Error> {
    let native = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT(desc.format as i32),
        ViewDimension: D3D11_UAV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_UAV {
                MipSlice: desc.mip_slice,
            },
        },
    };
    let mut uav: Option<ID3D11UnorderedAccessView> = None;
    unsafe {
        device
            .CreateUnorderedAccessView(buffer, Some(&native), Some(&mut uav))
            .map_err(|e| Error::windows("ID3D11Device::CreateUnorderedAccessView", e))?;
    }
    uav.ok_or(Error::MissingObject("ID3D11UnorderedAccessView"))
}

trait OrContext {
    fn or_context(self, call: &'static str) -> Self;
}

impl OrContext for Error {
    fn or_context(self, call: &'static str) -> Self {
        match self {
            Error::Windows { source, .. } => Error::windows(call, source),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{
        FEATURE_LEVEL_10_0, FEATURE_LEVEL_10_1, FEATURE_LEVEL_11_0, FEATURE_LEVEL_11_1,
        FEATURE_LEVEL_9_1, FEATURE_LEVEL_9_2, FEATURE_LEVEL_9_3,
    };

    #[test]
    fn every_pinned_level_has_its_d3d_name() {
        let levels = [
            FEATURE_LEVEL_11_1,
            FEATURE_LEVEL_11_0,
            FEATURE_LEVEL_10_1,
            FEATURE_LEVEL_10_0,
            FEATURE_LEVEL_9_3,
            FEATURE_LEVEL_9_2,
            FEATURE_LEVEL_9_1,
        ];
        for level in levels {
            assert!(
                !feature_level_name(level).starts_with("unknown"),
                "0x{level:04x} must name a D3D_FEATURE_LEVEL"
            );
        }
        assert!(feature_level_name(0x1234).starts_with("unknown"));
    }

    #[test]
    fn the_stage_and_clear_are_declared_not_client_values() {
        assert_eq!(STAGE, "D1");
        assert_eq!(VIEW_STAGE, "D2");
        assert_eq!(STAGES, "D1+D2");
        // A D1 placeholder: no client clear constant is read.
        assert_eq!(D1_CLEAR_COLOUR, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn the_view_description_prints_every_read_field() {
        let view = crate::request::SwapChainViewRequest::client_default();
        let desc = SwapChainViewDescription {
            width: 1280,
            height: 720,
            engine_format: 0x1f,
            view_format: 28,
            usage_bits: 1,
            render_target: true,
            shader_resource: true,
            unordered_access: false,
            rtv: view.rtv_desc().unwrap(),
            srv: view.srv_desc(),
            uav: view.uav_desc(),
        };
        let text = desc.to_string();
        assert!(text.contains("1280x720"), "{text}");
        assert!(
            text.contains("engine format 0x1f, view format 28"),
            "{text}"
        );
        assert!(text.contains("RTV format=28 dim=4 mip=0"), "{text}");
        assert!(
            text.contains("SRV format=28 dim=4 mip=0 levels=1"),
            "{text}"
        );
        assert!(text.contains("UAV absent"), "{text}");
        assert_eq!(desc.srv.unwrap().mip_levels, 1);
        assert_eq!(desc.uav, None);
    }
}

//! The client's D3D11 creation request, as plain values.
//!
//! Every constant here is read from the client's own code in the complete
//! `Overwatch.exe` Ghidra image; the addresses and the operation-by-operation
//! conclusions live in `docs/formats/renderer.md` (stages D1/D2). This module
//! deliberately carries no COM type: the values can be pinned by unit tests on
//! any machine, including one with no usable GPU.
//!
//! **What is exact here.** The feature-level lists, the SDK version, the
//! creation-flag expression, the default back-buffer format, count and usage,
//! and the swap-chain field expressions. **What is not.** The three boolean
//! inputs to the creation-flag expression, and the sync-interval byte, are
//! config values the shipped build supplies at runtime; their defaults are not
//! read (see `docs/formats/renderer.md`, "What D1 proves").

// ---------------------------------------------------------------------------
// D3D11CreateDevice
// ---------------------------------------------------------------------------

/// `D3D11_SDK_VERSION` passed to `D3D11CreateDevice`.
pub const SDK_VERSION: u32 = 7;

/// `D3D_FEATURE_LEVEL_11_1`.
pub const FEATURE_LEVEL_11_1: u32 = 0xb100;
/// `D3D_FEATURE_LEVEL_11_0`.
pub const FEATURE_LEVEL_11_0: u32 = 0xb000;
/// `D3D_FEATURE_LEVEL_10_1`.
pub const FEATURE_LEVEL_10_1: u32 = 0xa100;
/// `D3D_FEATURE_LEVEL_10_0`.
pub const FEATURE_LEVEL_10_0: u32 = 0xa000;
/// `D3D_FEATURE_LEVEL_9_3`.
pub const FEATURE_LEVEL_9_3: u32 = 0x9300;
/// `D3D_FEATURE_LEVEL_9_2`.
pub const FEATURE_LEVEL_9_2: u32 = 0x9200;
/// `D3D_FEATURE_LEVEL_9_1`.
pub const FEATURE_LEVEL_9_1: u32 = 0x9100;

/// The first request: exactly one level, `11_1`.
///
/// The client calls `D3D11CreateDevice` with a one-element array holding
/// `0xb100`; a failure is what triggers the fallback list.
pub const FEATURE_LEVEL_PRIMARY: &[u32] = &[FEATURE_LEVEL_11_1];

/// The retry list, in the client's order.
///
/// It contains six levels and deliberately **drops** `11_1`: the retry is the
/// pre-Windows-8.1 path, where asking for `11_1` again would fail the same way.
pub const FEATURE_LEVEL_FALLBACK: &[u32] = &[
    FEATURE_LEVEL_11_0,
    FEATURE_LEVEL_10_1,
    FEATURE_LEVEL_10_0,
    FEATURE_LEVEL_9_3,
    FEATURE_LEVEL_9_2,
    FEATURE_LEVEL_9_1,
];

/// `D3D11_CREATE_DEVICE_SINGLETHREADED`.
pub const CREATE_DEVICE_SINGLETHREADED: u32 = 0x1;
/// `D3D11_CREATE_DEVICE_DEBUG`.
pub const CREATE_DEVICE_DEBUG: u32 = 0x2;
/// `D3D11_CREATE_DEVICE_PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY`.
pub const CREATE_DEVICE_PREVENT_ALTERING_LAYER: u32 = 0x80;

/// The three read inputs of the client's creation-flag expression.
///
/// The names describe the read condition, not a config key:
/// - `debug_layer` is the non-zero dword at `pm_dx11::Instance::Init`'s request
///   `+0x1c` (copied to instance `+0x34`); a zero value clears DEBUG.
/// - `single_threaded` is the non-zero byte at the device-creation descriptor
///   `+0x40` passed to `pm_dx11::Instance::CreateDevice`.
/// - `allow_layer_altering` is the non-zero byte at the instance request
///   `+0x2c` (copied to instance `+0x44`). The client sets
///   PREVENT_ALTERING_LAYER when this byte is **zero**, so `false` sets it.
///
/// The shipped build's values for all three are **not read**; only the
/// expression that turns them into the flag word is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceRequest {
    /// `D3D11_CREATE_DEVICE_DEBUG` when true.
    pub debug_layer: bool,
    /// `D3D11_CREATE_DEVICE_SINGLETHREADED` when true.
    pub single_threaded: bool,
    /// `D3D11_CREATE_DEVICE_PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY`
    /// when **false**.
    pub allow_layer_altering: bool,
    /// When set (the instance's `+0xcc` dword is non-zero), the client asks for
    /// this single level and does not retry. The engine stores a raw value
    /// here; the read path that produces a non-zero value is not seen in this
    /// build, so no shipped value is claimed.
    pub feature_level_override: Option<u32>,
}

impl DeviceRequest {
    /// The client's flag expression, exactly:
    /// DEBUG if the debug input is non-zero, SINGLETHREADED if that input is
    /// non-zero, and PREVENT_ALTERING_LAYER when the layer input is zero.
    pub const fn creation_flags(&self) -> u32 {
        let mut flags = if self.debug_layer {
            CREATE_DEVICE_DEBUG
        } else {
            0
        };
        if self.single_threaded {
            flags |= CREATE_DEVICE_SINGLETHREADED;
        }
        if !self.allow_layer_altering {
            flags |= CREATE_DEVICE_PREVENT_ALTERING_LAYER;
        }
        flags
    }

    /// The levels to pass on the first call: the override alone, or `11_1`.
    pub fn primary_feature_levels(&self) -> Vec<u32> {
        match self.feature_level_override {
            Some(level) => vec![level],
            None => FEATURE_LEVEL_PRIMARY.to_vec(),
        }
    }

    /// The levels to pass after a failed first call, or `None` when an
    /// override makes the client skip the retry.
    pub fn fallback_feature_levels(&self) -> Option<Vec<u32>> {
        match self.feature_level_override {
            Some(_) => None,
            None => Some(FEATURE_LEVEL_FALLBACK.to_vec()),
        }
    }
}

// ---------------------------------------------------------------------------
// The swap chain
// ---------------------------------------------------------------------------

/// `DXGI_FORMAT_R8G8B8A8_UNORM`, the default back-buffer format.
///
/// The engine view creates the swap chain with engine format index `0x1f`;
/// `pm_dxgi::Instance::InitSwapChain` resolves that index to DXGI 28.
pub const BACK_BUFFER_FORMAT: u32 = 28;
/// The engine view's format byte for the default view (`view+0x4c`, request
/// `+0x2c`). `FUN_141ef6960` writes it; `FUN_142636500` is the alternative
/// producer and is **open**.
pub const DEFAULT_ENGINE_FORMAT: u8 = 0x1f;
/// The engine view's buffer count (`view+0x50`).
pub const BACK_BUFFER_COUNT: u32 = 3;
/// `DXGI_USAGE_RENDER_TARGET_OUTPUT | DXGI_USAGE_SHADER_INPUT`.
///
/// The engine view sets usage bit 0 (`view+0x60 |= 1`), which adds
/// `DXGI_USAGE_SHADER_INPUT` to the base `RENDER_TARGET_OUTPUT`. Usage bit 1
/// adds `DXGI_USAGE_UNORDERED_ACCESS` (`0x400`); the default view leaves it
/// clear.
pub const BACK_BUFFER_USAGE: u32 = 0x30;
/// The default swap-effect value: the `DXGI_SWAP_CHAIN_DESC1` field is built
/// as `2` when the request's `+0x28` bit 0 is set, otherwise `0`. The shipped
/// view request leaves `+0x28` zero.
pub const SWAP_EFFECT_DEFAULT: u32 = 0;
/// The flip-model swap-effect value the client uses on the newer factory path
/// (`4`, `DXGI_SWAP_EFFECT_FLIP_DISCARD`), reached only when the tearing/HDR
/// branch is taken. It is pinned because the branch is read; the default path
/// does not use it.
pub const SWAP_EFFECT_FLIP: u32 = 4;
/// `DXGI_SCALING_STRETCH`, the default scaling value.
pub const SCALING_STRETCH: u32 = 0;
/// `DXGI_ALPHA_MODE_UNSPECIFIED`.
pub const ALPHA_MODE_UNSPECIFIED: u32 = 0;
/// `DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH`, set when request `+0x28` bit 0 is
/// set. The default view request does not set it.
pub const SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH: u32 = 0x2;
/// `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`, set on the read
/// tearing path.
pub const SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT: u32 = 0x40;
/// The additional flag the client sets on the newer-factory flip path when
/// `instance+0xb0` is non-zero. The enum member is not named by the read code;
/// the value is pinned, the name is **open**.
pub const SWAP_CHAIN_FLAG_0X800: u32 = 0x800;

/// `DXGI_SWAP_CHAIN_DESC1` as the client builds it, in plain words.
///
/// Width and height are the engine view's requested size (`request+0x20` /
/// `+0x24`). The format is resolved from the engine index (`+0x2c`) through
/// `0x14348bf10`, with the read special cases `0x20 -> 28` and `0x25 -> 87`.
/// The default view uses index `0x1f`, which resolves to 28.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapChainRequest {
    pub width: u32,
    pub height: u32,
    /// `DXGI_FORMAT` value.
    pub format: u32,
    /// The engine view's raw format byte (`request+0x2c`). The swap-chain
    /// `format` and the D3D11 view format are resolved from it by two
    /// different tables; [`client_view`](SwapChainRequest::client_view) keeps
    /// the two coherent.
    pub engine_format: u8,
    pub buffer_count: u32,
    /// `DXGI_USAGE` bits.
    pub buffer_usage: u32,
    /// `DXGI_SWAP_EFFECT` value (0 on the default path, 4 on the flip path).
    pub swap_effect: u32,
    /// `DXGI_SCALING` value.
    pub scaling: u32,
    /// `DXGI_ALPHA_MODE` value.
    pub alpha_mode: u32,
    /// `DXGI_SWAP_CHAIN_DESC1::Flags` bits.
    pub flags: u32,
    /// `Present`'s first argument. The client copies the view's boolean
    /// `view+0x54` into the present object's `+0x60`; the producer of that
    /// boolean is a runtime config global and is **open**, so the value is a
    /// parameter here, not a claimed default.
    pub sync_interval: u32,
}

impl SwapChainRequest {
    /// The default engine view: 3 buffers, DXGI 28, render-target plus shader
    /// input usage, discard-model swap effect, stretch scaling, no flags.
    ///
    /// `sync_interval` is supplied because the client's value is a boolean from
    /// an unresolved config global (0 or 1).
    pub const fn client_default(width: u32, height: u32, sync_interval: u32) -> Self {
        Self {
            width,
            height,
            format: BACK_BUFFER_FORMAT,
            engine_format: DEFAULT_ENGINE_FORMAT,
            buffer_count: BACK_BUFFER_COUNT,
            buffer_usage: BACK_BUFFER_USAGE,
            swap_effect: SWAP_EFFECT_DEFAULT,
            scaling: SCALING_STRETCH,
            alpha_mode: ALPHA_MODE_UNSPECIFIED,
            flags: 0,
            sync_interval,
        }
    }

    /// The client's view request for an explicit engine format byte and usage
    /// bits, with both formats resolved the way the client resolves them.
    ///
    /// `None` means the format byte points outside the 64-entry table the
    /// client reads; the client has no range check there (see
    /// [`view_format`](crate::request::view_format)).
    pub fn client_view(
        width: u32,
        height: u32,
        sync_interval: u32,
        engine_format: u8,
        usage_bits: u8,
    ) -> Option<Self> {
        Some(Self {
            width,
            height,
            format: swap_chain_format(engine_format)?,
            engine_format,
            buffer_count: BACK_BUFFER_COUNT,
            buffer_usage: Self::buffer_usage_from_bits(usage_bits),
            swap_effect: SWAP_EFFECT_DEFAULT,
            scaling: SCALING_STRETCH,
            alpha_mode: ALPHA_MODE_UNSPECIFIED,
            flags: 0,
            sync_interval,
        })
    }

    /// The view request implied by this swap-chain request. Usage bit 0 is
    /// carried by `DXGI_USAGE_SHADER_INPUT` (`0x10`), bit 1 by
    /// `DXGI_USAGE_UNORDERED_ACCESS` (`0x400`).
    pub const fn view_request(&self) -> SwapChainViewRequest {
        SwapChainViewRequest {
            engine_format: self.engine_format,
            usage_bits: usage_bits_from_buffer_usage(self.buffer_usage),
        }
    }

    /// The client's `BufferUsage` expression from request `+0x40`:
    /// `0x20` base, `+0x10` when bit 0 is set, `+0x400` when bit 1 is set.
    pub const fn buffer_usage_from_bits(bits: u8) -> u32 {
        let mut usage = 0x20;
        if bits & 1 != 0 {
            usage |= 0x10;
        }
        if bits & 2 != 0 {
            usage |= 0x400;
        }
        usage
    }

    /// The client's `Flags` expression for the `DESC1` path: bit 0 of request
    /// `+0x28` adds `ALLOW_MODE_SWITCH`; the read tearing path adds
    /// `FRAME_LATENCY_WAITABLE_OBJECT`, and the newer-factory flip path adds
    /// the unnamed `0x800`.
    pub const fn flags_from_bits(mode_switch: bool, tearing: bool, flip_latency: bool) -> u32 {
        let mut flags = 0;
        if mode_switch {
            flags |= SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH;
        }
        if tearing {
            flags |= SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT;
        }
        if flip_latency {
            flags |= SWAP_CHAIN_FLAG_0X800;
        }
        flags
    }
}

// ---------------------------------------------------------------------------
// D2: swap-chain views
// ---------------------------------------------------------------------------

/// The client's engine-format → DXGI-format table, 64 dwords, read from
/// `0x1434965d0` (the D3D11 view format) and byte-identical to the
/// swap-chain buffer table at `0x14348bf10`.
///
/// `pm_dx11::Device::CreateSwapChainResources` (`0x14266a060`) indexes this
/// table with the engine format byte (`request+0x2c`), after six explicit
/// special cases. The client performs **no range check**: a byte `>= 64`
/// would read past the table. No shipped swap-chain path produces one, so
/// [`view_format`] returns `None` rather than inventing the adjacent memory.
pub const VIEW_FORMAT_TABLE: [u32; 64] = [
    0, 61, 0, 63, 62, 64, 65, 56, 58, 57, 59, 54, 49, 0, 51, 50, 52, 0, 115, 0, 85, 0, 86, 42, 43,
    41, 35, 37, 36, 38, 34, 28, 29, 31, 30, 32, 87, 91, 24, 25, 0, 26, 67, 17, 18, 16, 11, 13, 12,
    14, 10, 7, 8, 6, 3, 4, 2, 71, 72, 74, 75, 77, 78, 80,
];

/// The six engine format bytes the client handles explicitly in
/// `CreateSwapChainResources`, before the table lookup.
pub const VIEW_FORMAT_SPECIAL_CASES: [(u8, u32); 6] = [
    (0x83, 0x38),
    (0x84, 0x29),
    (0x85, 0x2e),
    (0x86, 0x2e),
    (0x87, 0x15),
    (0x88, 0x3d),
];

/// Resolve the engine format byte to the client's D3D11 **view** format.
///
/// Order, as read: the six special cases first, then
/// `VIEW_FORMAT_TABLE[byte]`. The default view's `0x1f` gives 28.
pub fn view_format(engine_format: u8) -> Option<u32> {
    for (index, format) in VIEW_FORMAT_SPECIAL_CASES {
        if engine_format == index {
            return Some(format);
        }
    }
    VIEW_FORMAT_TABLE.get(engine_format as usize).copied()
}

/// Resolve the engine format byte to the **swap-chain buffer** format.
///
/// Same table and the same read values as [`view_format`], with the client's
/// two buffer-only special cases (`0x20 -> 28`, `0x25 -> 87`). The table at
/// `0x14348bf10` is byte-identical to the one at `0x1434965d0`.
pub fn swap_chain_format(engine_format: u8) -> Option<u32> {
    match engine_format {
        0x20 => Some(28),
        0x25 => Some(87),
        other => view_format(other),
    }
}

/// `D3D11_RTV_DIMENSION_TEXTURE2D` (the client stores view dimension 4).
pub const RTV_DIMENSION_TEXTURE2D: u32 = 4;
/// `D3D11_SRV_DIMENSION_TEXTURE2D` (the client stores view dimension 4).
pub const SRV_DIMENSION_TEXTURE2D: u32 = 4;
/// `D3D11_UAV_DIMENSION_TEXTURE2D` (the client stores view dimension 4).
pub const UAV_DIMENSION_TEXTURE2D: u32 = 4;
/// The RTV/UAV union's texture-2D mip slice. The client stores zero.
pub const VIEW_MIP_SLICE: u32 = 0;
/// The SRV union's most detailed mip. The client stores zero.
pub const SRV_MOST_DETAILED_MIP: u32 = 0;
/// The SRV union's mip-level count. The client stores one.
pub const SRV_MIP_LEVELS: u32 = 1;

/// `DXGI_USAGE_SHADER_INPUT`; engine usage bit 0.
pub const USAGE_SHADER_INPUT: u32 = 0x10;
/// `DXGI_USAGE_UNORDERED_ACCESS`; engine usage bit 1.
pub const USAGE_UNORDERED_ACCESS: u32 = 0x400;

/// `DXGI_PRESENT_ALLOW_TEARING`, the only present flag the client sets.
pub const PRESENT_ALLOW_TEARING: u32 = 0x200;

/// `pm_dxgi::Instance::Present`'s flag expression (`0x142630790`).
///
/// The flag is `DXGI_PRESENT_ALLOW_TEARING` only when **all four** read
/// conditions hold: the instance tearing input (`instance+0xb0`) is non-zero,
/// the sync interval (`present+0x60`) is zero, present `+0x48` is zero and
/// present `+0x49` is non-zero. Otherwise the flag word is zero.
pub const fn present_flags(
    instance_tearing: bool,
    sync_interval: u32,
    present_48: bool,
    present_49: bool,
) -> u32 {
    if instance_tearing && sync_interval == 0 && !present_48 && present_49 {
        PRESENT_ALLOW_TEARING
    } else {
        0
    }
}

/// The engine view fields that decide a swap-chain view: the format byte and
/// the two usage bits read at request `+0x40`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapChainViewRequest {
    pub engine_format: u8,
    /// Bit 0 creates an SRV, bit 1 a UAV; the other bits are not read by the
    /// swap-chain resource function.
    pub usage_bits: u8,
}

/// Recover the engine usage bits from the swap chain's `DXGI_USAGE` word:
/// bit 0 from `DXGI_USAGE_SHADER_INPUT`, bit 1 from
/// `DXGI_USAGE_UNORDERED_ACCESS`.
pub const fn usage_bits_from_buffer_usage(buffer_usage: u32) -> u8 {
    let mut bits = 0;
    if buffer_usage & USAGE_SHADER_INPUT != 0 {
        bits |= 1;
    }
    if buffer_usage & USAGE_UNORDERED_ACCESS != 0 {
        bits |= 2;
    }
    bits
}

impl SwapChainViewRequest {
    /// The default engine view: format byte `0x1f`, usage bit 0 only.
    pub const fn client_default() -> Self {
        Self {
            engine_format: DEFAULT_ENGINE_FORMAT,
            usage_bits: 1,
        }
    }

    /// The resolved view format, or `None` for a byte outside the read table.
    pub fn view_format(&self) -> Option<u32> {
        view_format(self.engine_format)
    }

    /// The client creates an SRV when usage bit 0 is set.
    pub const fn creates_srv(&self) -> bool {
        self.usage_bits & 1 != 0
    }

    /// The client creates a UAV when usage bit 1 is set.
    pub const fn creates_uav(&self) -> bool {
        self.usage_bits & 2 != 0
    }

    /// The explicit render-target view descriptor the client passes:
    /// resolved view format, `D3D11_RTV_DIMENSION_TEXTURE2D`, mip slice 0.
    pub fn rtv_desc(&self) -> Option<RenderTargetViewDesc> {
        Some(RenderTargetViewDesc {
            format: self.view_format()?,
            view_dimension: RTV_DIMENSION_TEXTURE2D,
            mip_slice: VIEW_MIP_SLICE,
        })
    }

    /// The explicit shader-resource view descriptor, when usage bit 0 is set:
    /// resolved view format, `D3D11_SRV_DIMENSION_TEXTURE2D`,
    /// most-detailed mip 0, one mip level.
    pub fn srv_desc(&self) -> Option<ShaderResourceViewDesc> {
        if !self.creates_srv() {
            return None;
        }
        Some(ShaderResourceViewDesc {
            format: self.view_format()?,
            view_dimension: SRV_DIMENSION_TEXTURE2D,
            most_detailed_mip: SRV_MOST_DETAILED_MIP,
            mip_levels: SRV_MIP_LEVELS,
        })
    }

    /// The explicit unordered-access view descriptor, when usage bit 1 is
    /// set: resolved view format, `D3D11_UAV_DIMENSION_TEXTURE2D`, mip slice
    /// 0.
    pub fn uav_desc(&self) -> Option<UnorderedAccessViewDesc> {
        if !self.creates_uav() {
            return None;
        }
        Some(UnorderedAccessViewDesc {
            format: self.view_format()?,
            view_dimension: UAV_DIMENSION_TEXTURE2D,
            mip_slice: VIEW_MIP_SLICE,
        })
    }
}

/// `D3D11_RENDER_TARGET_VIEW_DESC` for a texture-2D view, as plain values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTargetViewDesc {
    pub format: u32,
    pub view_dimension: u32,
    pub mip_slice: u32,
}

/// `D3D11_SHADER_RESOURCE_VIEW_DESC` for a texture-2D view, as plain values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderResourceViewDesc {
    pub format: u32,
    pub view_dimension: u32,
    pub most_detailed_mip: u32,
    pub mip_levels: u32,
}

/// `D3D11_UNORDERED_ACCESS_VIEW_DESC` for a texture-2D view, as plain values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnorderedAccessViewDesc {
    pub format: u32,
    pub view_dimension: u32,
    pub mip_slice: u32,
}

/// The client's release order for a swap-chain resource object
/// (`FUN_14266bd90`, called before `ResizeBuffers`): the back-buffer texture,
/// then the RTV, then the SRV, then the UAV. Each is released only when
/// present.
pub const SWAP_RESOURCE_RELEASE_ORDER: [SwapResource; 4] = [
    SwapResource::Texture,
    SwapResource::RenderTargetView,
    SwapResource::ShaderResourceView,
    SwapResource::UnorderedAccessView,
];

/// One object in a swap-chain resource set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapResource {
    Texture,
    RenderTargetView,
    ShaderResourceView,
    UnorderedAccessView,
}

/// The client's resize order for the views:
/// release texture/RTV/SRV/UAV, `ResizeBuffers`, then
/// `CreateSwapChainResources` again (GetBuffer, RTV, optional SRV, optional
/// UAV).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewStep {
    Release(SwapResource),
    ResizeBuffers,
    GetBuffer,
    CreateRenderTargetView,
    CreateShaderResourceView,
    CreateUnorderedAccessView,
}

impl SwapChainViewRequest {
    /// The read resize sequence, with the optional create steps present only
    /// when the client's usage gates them.
    pub fn resize_steps(&self) -> Vec<ViewStep> {
        let mut steps: Vec<ViewStep> = SWAP_RESOURCE_RELEASE_ORDER
            .iter()
            .copied()
            .map(ViewStep::Release)
            .collect();
        steps.push(ViewStep::ResizeBuffers);
        steps.push(ViewStep::GetBuffer);
        steps.push(ViewStep::CreateRenderTargetView);
        if self.creates_srv() {
            steps.push(ViewStep::CreateShaderResourceView);
        }
        if self.creates_uav() {
            steps.push(ViewStep::CreateUnorderedAccessView);
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_version_is_seven() {
        assert_eq!(SDK_VERSION, 7);
    }

    #[test]
    fn the_first_request_is_11_1_alone() {
        let request = DeviceRequest::default();
        assert_eq!(request.primary_feature_levels(), vec![0xb100]);
        assert_eq!(FEATURE_LEVEL_PRIMARY.len(), 1);
    }

    #[test]
    fn the_fallback_list_is_the_clients_six_levels_in_order() {
        let levels = DeviceRequest::default()
            .fallback_feature_levels()
            .expect("the default request retries");
        assert_eq!(levels, vec![0xb000, 0xa100, 0xa000, 0x9300, 0x9200, 0x9100]);
        assert!(
            !levels.contains(&FEATURE_LEVEL_11_1),
            "the retry drops 11_1"
        );
    }

    #[test]
    fn an_override_replaces_the_list_and_skips_the_retry() {
        let request = DeviceRequest {
            feature_level_override: Some(0xb000),
            ..DeviceRequest::default()
        };
        assert_eq!(request.primary_feature_levels(), vec![0xb000]);
        assert_eq!(request.fallback_feature_levels(), None);
    }

    #[test]
    fn creation_flags_follow_the_read_expression() {
        // (debug, single_threaded, allow_layer_altering) -> flag word.
        let cases = [
            (false, false, false, 0x80),
            (false, false, true, 0x00),
            (false, true, false, 0x81),
            (false, true, true, 0x01),
            (true, false, false, 0x82),
            (true, false, true, 0x02),
            (true, true, false, 0x83),
            (true, true, true, 0x03),
        ];
        for (debug, single, allow, expected) in cases {
            let request = DeviceRequest {
                debug_layer: debug,
                single_threaded: single,
                allow_layer_altering: allow,
                feature_level_override: None,
            };
            assert_eq!(
                request.creation_flags(),
                expected,
                "debug={debug} single={single} allow={allow}"
            );
        }
    }

    #[test]
    fn the_default_view_request_pins_the_read_values() {
        let request = SwapChainRequest::client_default(1280, 720, 1);
        assert_eq!(request.width, 1280);
        assert_eq!(request.height, 720);
        assert_eq!(request.format, 28);
        assert_eq!(request.engine_format, 0x1f);
        assert_eq!(request.buffer_count, 3);
        assert_eq!(request.buffer_usage, 0x30);
        assert_eq!(request.swap_effect, 0);
        assert_eq!(request.scaling, 0);
        assert_eq!(request.alpha_mode, 0);
        assert_eq!(request.flags, 0);
        assert_eq!(request.sync_interval, 1);
    }

    #[test]
    fn usage_bits_are_the_clients_expression() {
        assert_eq!(SwapChainRequest::buffer_usage_from_bits(0), 0x20);
        assert_eq!(SwapChainRequest::buffer_usage_from_bits(1), 0x30);
        assert_eq!(SwapChainRequest::buffer_usage_from_bits(2), 0x420);
        assert_eq!(SwapChainRequest::buffer_usage_from_bits(3), 0x430);
        assert_eq!(
            SwapChainRequest::client_default(1, 1, 0).buffer_usage,
            SwapChainRequest::buffer_usage_from_bits(1)
        );
    }

    #[test]
    fn swap_chain_flags_are_the_clients_expression() {
        assert_eq!(SwapChainRequest::flags_from_bits(false, false, false), 0);
        assert_eq!(SwapChainRequest::flags_from_bits(true, false, false), 0x2);
        assert_eq!(SwapChainRequest::flags_from_bits(false, true, false), 0x40);
        assert_eq!(SwapChainRequest::flags_from_bits(false, false, true), 0x800);
        assert_eq!(
            SwapChainRequest::flags_from_bits(true, true, true),
            0x2 | 0x40 | 0x800
        );
    }

    // -----------------------------------------------------------------------
    // D2: the view format table and descriptors
    // -----------------------------------------------------------------------

    #[test]
    fn the_view_format_table_is_pinned_entry_for_entry() {
        // Read from 0x1434965d0; the table at 0x14348bf10 is byte-identical.
        #[rustfmt::skip]
        let client = [
            0, 61, 0, 63, 62, 64, 65, 56, 58, 57, 59, 54, 49, 0, 51, 50,
            52, 0, 115, 0, 85, 0, 86, 42, 43, 41, 35, 37, 36, 38, 34, 28,
            29, 31, 30, 32, 87, 91, 24, 25, 0, 26, 67, 17, 18, 16, 11, 13,
            12, 14, 10, 7, 8, 6, 3, 4, 2, 71, 72, 74, 75, 77, 78, 80,
        ];
        assert_eq!(VIEW_FORMAT_TABLE.len(), 64);
        assert_eq!(VIEW_FORMAT_TABLE, client);
    }

    #[test]
    fn the_default_view_format_is_dxgi_28() {
        assert_eq!(view_format(0x1f), Some(28));
        assert_eq!(view_format(0x20), Some(29), "0x20 is the sRGB view");
        assert_eq!(view_format(0x21), Some(31));
        assert_eq!(swap_chain_format(0x1f), Some(28));
        assert_eq!(
            swap_chain_format(0x20),
            Some(28),
            "the buffer table maps 0x20 to UNORM while the view table maps it to sRGB"
        );
        assert_eq!(swap_chain_format(0x25), Some(87));
    }

    #[test]
    fn the_six_explicit_special_cases_are_pinned() {
        assert_eq!(VIEW_FORMAT_SPECIAL_CASES.len(), 6);
        assert_eq!(view_format(0x83), Some(0x38));
        assert_eq!(view_format(0x84), Some(0x29));
        assert_eq!(view_format(0x85), Some(0x2e));
        assert_eq!(view_format(0x86), Some(0x2e));
        assert_eq!(view_format(0x87), Some(0x15));
        assert_eq!(view_format(0x88), Some(0x3d));
        for (index, format) in VIEW_FORMAT_SPECIAL_CASES {
            assert_eq!(view_format(index), Some(format), "index 0x{index:02x}");
        }
    }

    #[test]
    fn a_format_byte_outside_the_table_is_declared_unread() {
        // The client indexes with no range check; the read table stops at 63
        // and no shipped swap-chain view produces a larger byte.
        assert_eq!(view_format(64), None);
        assert_eq!(view_format(0x88 + 1), None);
        assert_eq!(view_format(0xff), None);
        assert_eq!(swap_chain_format(0x40), None);
    }

    #[test]
    fn the_default_view_creates_a_render_target_and_a_shader_resource_only() {
        let view = SwapChainViewRequest::client_default();
        assert_eq!(view.engine_format, 0x1f);
        assert_eq!(view.usage_bits, 1);
        assert!(view.creates_srv());
        assert!(!view.creates_uav());
        assert_eq!(
            view.rtv_desc(),
            Some(RenderTargetViewDesc {
                format: 28,
                view_dimension: RTV_DIMENSION_TEXTURE2D,
                mip_slice: 0,
            })
        );
        assert_eq!(
            view.srv_desc(),
            Some(ShaderResourceViewDesc {
                format: 28,
                view_dimension: SRV_DIMENSION_TEXTURE2D,
                most_detailed_mip: 0,
                mip_levels: 1,
            })
        );
        assert_eq!(view.uav_desc(), None);
    }

    #[test]
    fn usage_bit_one_creates_the_unordered_access_view() {
        let view = SwapChainViewRequest {
            engine_format: 0x1f,
            usage_bits: 3,
        };
        assert_eq!(
            view.uav_desc(),
            Some(UnorderedAccessViewDesc {
                format: 28,
                view_dimension: UAV_DIMENSION_TEXTURE2D,
                mip_slice: 0,
            })
        );
        let uav_only = SwapChainViewRequest {
            engine_format: 0x1f,
            usage_bits: 2,
        };
        assert_eq!(uav_only.srv_desc(), None);
        assert!(uav_only.uav_desc().is_some());
    }

    #[test]
    fn the_usage_bits_recover_from_the_swap_chain_usage_word() {
        assert_eq!(usage_bits_from_buffer_usage(0x20), 0);
        assert_eq!(usage_bits_from_buffer_usage(0x30), 1);
        assert_eq!(usage_bits_from_buffer_usage(0x420), 2);
        assert_eq!(usage_bits_from_buffer_usage(0x430), 3);
        assert_eq!(
            SwapChainRequest::client_default(1, 1, 0)
                .view_request()
                .usage_bits,
            1
        );
    }

    #[test]
    fn the_client_view_builder_keeps_both_formats_coherent() {
        let default = SwapChainRequest::client_view(1280, 720, 1, 0x1f, 1).expect("read format");
        assert_eq!(
            default,
            SwapChainRequest::client_default(1280, 720, 1),
            "the default view is the explicit builder with 0x1f and usage bit 0"
        );
        let srgb_view = SwapChainRequest::client_view(1, 1, 0, 0x20, 1).expect("read format");
        assert_eq!(srgb_view.format, 28, "the buffer stays UNORM");
        assert_eq!(
            srgb_view.view_request().view_format(),
            Some(29),
            "the view is sRGB over the UNORM buffer"
        );
        assert_eq!(SwapChainRequest::client_view(1, 1, 0, 0xff, 0), None);
    }

    #[test]
    fn present_flags_follow_the_clients_four_conditions() {
        assert_eq!(present_flags(false, 0, false, true), 0);
        assert_eq!(present_flags(true, 1, false, true), 0);
        assert_eq!(present_flags(true, 0, true, true), 0);
        assert_eq!(present_flags(true, 0, false, false), 0);
        assert_eq!(present_flags(true, 0, false, true), PRESENT_ALLOW_TEARING);
        // The default view leaves the tearing path clear.
        assert_eq!(present_flags(false, 1, false, false), 0);
    }

    #[test]
    fn the_resize_steps_are_the_clients_release_and_recreate_order() {
        let default = SwapChainViewRequest::client_default();
        assert_eq!(
            default.resize_steps(),
            vec![
                ViewStep::Release(SwapResource::Texture),
                ViewStep::Release(SwapResource::RenderTargetView),
                ViewStep::Release(SwapResource::ShaderResourceView),
                ViewStep::Release(SwapResource::UnorderedAccessView),
                ViewStep::ResizeBuffers,
                ViewStep::GetBuffer,
                ViewStep::CreateRenderTargetView,
                ViewStep::CreateShaderResourceView,
            ]
        );
        let uav_only = SwapChainViewRequest {
            engine_format: 0x1f,
            usage_bits: 2,
        };
        assert_eq!(
            uav_only.resize_steps().last().copied(),
            Some(ViewStep::CreateUnorderedAccessView)
        );
    }
}

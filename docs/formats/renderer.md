# The renderer

**D3D11 is the production path. D3D12 is the beta path.** The operator's
decision of 2026-10-31 fixes the 1:1 renderer target: the client's shipped
renderer runs on `pm_dx11` and the game labels its D3D12 backend beta. The
proof model is **the client's D3D11 code plus synthetic record-stream tests**.
A capture of the client is **impossible**: the anti-cheat makes attaching or
launching it under RenderDoc/PIX an account-ban risk, so there is no client
call trace and no client image oracle. RenderDoc may be used on **our own**
implementation only, and a self-capture is never a comparison against the
client. The D3D12 detail stays in this document as the secondary section for
the beta backend.

This is the map of the client's own renderer, read from the complete
`Overwatch.exe` image (`0x140000000` base). It is **not** an implementation and
it makes no gameplay claim. The `ow-client` viewer is our own `wgpu` renderer;
its shading is `FIT-PRE-02` in [`deviations.md`](../deviations.md). The native
`ow-render` crate now implements **stages D1-D4 and the D5 clear / draw /
viewport handlers**: the D3D11 device and swap chain, the back-buffer texture
with its RTV/SRV/UAV, the per-frame step order, the 64-opcode replay dispatch
and the read clear / draw / viewport handlers, each with the read values pinned
by synthetic tests. The D5 `0x0c` render-target set is implemented in the
executor and `ow-render::state`; `0x0d` is implemented as the pending-target
resolve, release and unbind.

The existing read pieces this map consumes and does not repeat:

- texture format, descriptor, upload order: [`materials.md`](materials.md),
  [`texture-1to1.md`](../texture-1to1.md);
- shader group / code / instance containers: [`shaders.md`](shaders.md);
- asset container: [`casc-1to1.md`](../casc-1to1.md);
- camera projection and basis: `fn/BuildPerspectiveMatrix`,
  `fn/InitProjectionDescriptor`, `fn/OrthonormalizeCameraBasis`.

Evidence classes are those of [`provenance.md`](../provenance.md): **read**,
**install**, **verified**, **open**. A negative is marked **negative**.

## Current state

- **D3D11 device and swap chain: read, implemented as D1, and self-captured.**
  The creation request, the flag expression, the feature-level lists, the
  adapter slot and the `DXGI_SWAP_CHAIN_DESC1` fields are pinned by
  `crates/ow-render` tests. The device and swap chain were created on this
  machine's RTX 4060 at `D3D_FEATURE_LEVEL_11_1`; a RenderDoc capture of our
  own path shows the 1280x720 `R8G8B8A8_UNORM` back buffer. A capture of the
  **client** is impossible under the anti-cheat, so there is no client
  comparison.
- **D3D11 swap-chain resources: read and implemented as D2.**
  `pm_dx11::Device::CreateSwapChainResources` (`0x14266a060`) is read
  operation for operation: `GetBuffer(0)`, the explicit texture-2D RTV
  descriptor, `GetDesc`, and the SRV/UAV gated by the two engine usage bits.
  The 64-entry view-format table at `0x1434965d0` and the six explicit
  special cases are pinned entry for entry. Resize is implemented in the read
  release → `ResizeBuffers` → recreate order. Our own capture shows the
  back-buffer texture; the client is not captured (anti-cheat).
- **D3D11 frame order: read and implemented as D3.** The frame-begin, wait and
  frame-end slots, the scene prepare, the queue flush/finish path, the
  `pm_dx11` submit and replay entry, the per-view submit and `Present` are
  read. `crates/ow-render::frame` holds the step list; only `Present` has a
  complete read body today, every other step is logged as **open** rather
  than guessed. Our own capture shows `ClearRenderTargetView` + `Present` as
  the only API calls, which matches the executed steps; the client is not
  captured (anti-cheat).
- **D3D11 shader objects, pipeline state and the command replay: read.**
  Addresses and operations are below. The replay dispatch accepts 64 opcodes.
  Eleven are implemented (nine self-contained handlers, including the `0x0c`
  render-target set and the `0x0d` pending resolve, plus the end record and the
  no-op loop-head entry); the other 53 are **open** by opcode with the read gap
  named. The executor and the
  recording sink are pinned by synthetic record streams.
- **Pass list: open.** There is no named render-graph or pass table in the
  image. The frame is recorded as a byte-stream command list. The replay names
  the colour clear (`0x07`), depth-stencil clear (`0x08`), render-target bind
  (`0x0c`), MSAA resolve + unbind (`0x0d`), viewport (`0x29`), scissor
  (`0x2a`), draw (`0x25`), indexed draw (`0x26`) and compute dispatch (`0x30`)
  opcodes. The bind is `OMSetRenderTargets` at `0x14266e54d`; `FUN_142674980`
  is a per-draw UAV/CB/SRV state flush called from `0x25`/`0x26`/`0x27`/`0x28`,
  not the bind. The world/GBuffer/lighting/post/UI pass sequence is still not
  read: the engine record stream that emits these opcodes is open. This is the
  central open item for both backends.
- **World draw submission: partly read.** The D3D11 replay has `Draw` /
  `DrawInstanced` (`0x25`) and `DrawIndexed` / `DrawIndexedInstanced` (`0x26`)
  with exact arguments; the draw-list build, visibility, sorting and
  per-object constants are open. The D3D12 device creates indirect draw
  command signatures.
- **Pipeline/shader selection: partly read.** The D3D11 shader object and
  pipeline create are read; the material → shader-group → code-GUID → tier
  selection is open.
- **Vertex/index layout: partly read.** The D3D11 pipeline builds an input
  layout from a vertex-element array and the shader input signature; the
  element record and the format table are read, but the material/model join
  that fills the elements is open. No shader-instance asset (`0x086`) exists
  in this build.
- **No renderer stage is verified against the client.** D1-D4's values,
  order, dispatch and the D5 clear/draw/viewport/`0x0c` handler arguments are
  exact and synthetically tested, including the `0x0c`/`0x0d` bind pair. They
  are not compared against a client call trace because a client capture is
  impossible under the anti-cheat; no pixel stage is provable.
  One command-append leaf can be driven by the current Unicorn oracle for its
  fixed-capacity domain only.

## The production path: D3D11

### Why the code is consistent with the decision

The platform's backend selector maps a request byte to a backend factory
(`FUN_1426377f0`, `0x1426377f0`): the value **2** selects the object built by
`FUN_142660e90` (`0x142660e90`), whose vtable is `0x143496d68` and whose Init
slot is `pm_dx11::Instance::Init` (`0x142673c80`). Every other value selects
the object built by `FUN_14263b140` (`0x14263b140`), whose vtable is
`0x1434904a8` and whose Init slot is `pm_dx12::Instance::Init`
(`0x14267b8b0`). `pm_api::PlatformInstance::CreateDevice` (`0x1426374b0`)
walks the enumerated GPUs and calls the selected backend for the first GPU that
accepts the device.

**What is read:** the kind → backend mapping, the vtable identity, and the full
`pm_dx11` method set. **What is not:** the engine setting that writes the kind
byte. The shipped default is not read here; the operator's decision fixes the
target, not a code value. The `pm_dx11` backend is complete, not a stub: it has
device creation, swap-chain resources, shader objects, pipeline state, buffer
and constant upload, a command queue and a 64-opcode replay.

The `pm_dx11` instance vtable is `0x143496d68`; its read slots are
`0x142663db0` (0), `pm_dx11::Instance::Init` `0x142673c80` (1),
`0x142665100` (2), the public `CreateDevice` `0x142666ca0` (3),
`0x142637650` (4), the recreate path `0x142675ac0` (5) and `0x1426361d0`
(6). The D3D12 vtable `0x1434904a8` has `pm_dx12::Instance::Init`
`0x14267b8b0` at slot 1 and `0x142645380` at slot 3.

### Layer stack

```text
engine renderer        0x141d00000 .. 0x141fxxxxx   frame, passes, draw batches
  pm_api               0x14263xxxx .. 0x14264xxxx   platform instance, backend choice
  pm_dx11              0x14265xxxx .. 0x14269xxxx   production device, pipeline, replay
  pm_dxgi              0x14262xxxx .. 0x14264xxxx   factory, adapters, swap chain, present
  pm_dx12              0x14263xxxx .. 0x14268xxxx   beta device, PSO, command lists
    d3d11.dll / d3d12.dll / dxgi.dll                loaded by name at runtime
```

### Imports are not the entry points

The PE import table has **14 entries** and none of them is a D3D or DXGI
function (`list_program_items(kind=imports)`). The graphics entry points are
resolved by name at runtime from strings; the module objects are built by small
loader functions.

| string | address | resolved function |
|---|---|---|
| `d3d11.dll` | `0x1434942b8`, `0x143497b98` | module table |
| `D3D11CreateDevice` | `0x1434942d0` | D3D11 device creation |
| `D3D11CreateDevice Failed` | `0x143497bf0` | create error |
| `d3d12.dll` | `0x1434987e8`, `0x1434987f8` | module table |
| `D3D12CreateDevice` | `0x143498828` | D3D12 device creation |
| `D3D12SerializeRootSignature` | `0x143498840` | root signature |
| `D3D12SerializeVersionedRootSignature` | `0x143498860` | root signature |
| `D3D12EnableExperimentalFeatures` | `0x143498888` | experimental features |
| `D3D12GetDebugInterface` | `0x1434988a8` | debug layer |
| `CreateDXGIFactory1` | `0x14348c1d0` | factory |
| `CreateDXGIFactory2` | `0x14348c208` | factory |
| `DXGIGetDebugInterface1` | `0x14348c...` | DXGI debug |
| `DXGIDeclareAdapterRemovalSupport` | resolved by name in `0x14262eaa0` | adapter removal |

**Negative:** `d3dcompiler` does not occur anywhere in the image (string search,
zero matches), and there is no import for it. Shaders are not compiled at
runtime by the client.

### D3D11 device creation

The chain, in call order:

| step | function | evidence |
|---|---|---|
| platform dispatch | `pm_api::PlatformInstance::CreateDevice` `0x1426374b0`; single-GPU path `0x1426372e0` | read: `Unsupported API Backend`, `CreateDevice Failed`, `No Compatible Gpus Found` |
| backend instance create | `FUN_1426377f0` `0x1426377f0` → `FUN_142660e90` `0x142660e90` | read: kind 2 builds vtable `0x143496d68` |
| `pm_dx11::Instance::Init` | `0x142673c80` | read: log `0x143497ba8`; loads `d3d11.dll` and resolves `D3D11CreateDevice` into instance `+0xd8` |
| public `CreateDevice` | `0x142666ca0` (vtable slot 3) | read: requires the device descriptor's size field `+0x68 == 0x140`, allocates a `0x4e8` device, calls `pm_dx11::Device::Init` |
| `pm_dx11::Device::Init` | `0x142673810` | read: log `0x143497c58`; calls the device create, sets maximum frame latency, debug name, optional deferred context |
| device request build | `FUN_142666980` `0x142666980` | read: logs `pm_dx11::Instance::CreateDevice` `0x143497c10`; owns the feature-level lists and the flag word |
| vendor wrapper | `FUN_14267d270` `0x14267d270` | read: calls the resolved `D3D11CreateDevice` with the read arguments |
| hardware probes | `FUN_14265ea70` `0x14265ea70`; `FUN_14267e5f0` `0x14267e5f0` | read: both create **temporary** devices for capability/hardware information; they are **not** the renderer's device |

`pm_dx11::Instance::Init` copies the neutral request at request `+0x18` through
`+0xf8` into instance `+0x30` through `+0xcc`. The fields D1 uses:

| request field | instance field | meaning |
|---|---|---|
| `+0x1c` u32 | `+0x34` | non-zero selects `D3D11_CREATE_DEVICE_DEBUG` |
| `+0x2c` u8 | `+0x44` | zero selects `D3D11_CREATE_DEVICE_PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY` |
| `+0xf8` u32 | `+0xcc` | non-zero replaces the feature-level list with this one value |
| `+0x18..` | `+0x30..` | adapter/LUID and the rest of the neutral request |

The device creation descriptor passed to `CreateDevice` supplies one more flag
input: its byte `+0x40` is non-zero for
`D3D11_CREATE_DEVICE_SINGLETHREADED`.

The flag word is exactly:

```text
flags = (debug_input != 0 ? 0x2 : 0)
      | (single_threaded_input != 0 ? 0x1 : 0)
      | (allow_layer_altering_input == 0 ? 0x80 : 0)
```

`0x2` is DEBUG, `0x1` SINGLETHREADED, `0x80`
PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY.

**Debug layer: negative.** No `D3D11GetDebugInterface` string or import exists
in the image. The D3D11 backend requests its debug layer only through the
`D3D11_CREATE_DEVICE_DEBUG` bit of the same `D3D11CreateDevice` call; there is
no separate debug-interface setup as there is for D3D12
(`D3D12GetDebugInterface`, `0x1434988a8`).

**Feature levels.** The first call asks for exactly one level,
`D3D_FEATURE_LEVEL_11_1` (`0xb100`). A failure triggers a single retry with six
levels in this order: `11_0` (`0xb000`), `10_1` (`0xa100`), `10_0` (`0xa000`),
`9_3` (`0x9300`), `9_2` (`0x9200`), `9_1` (`0x9100`). The retry deliberately
drops `11_1`. When the instance's `+0xcc` override is non-zero, the client
asks for that one value and does **not** retry.

**The `D3D11CreateDevice` arguments**, read at the vendor wrapper
(`0x14267d270`):

| argument | value |
|---|---|
| `pAdapter` | the platform GPU's DXGI adapter (`platform GPU +0xf0`) |
| `DriverType` | `D3D_DRIVER_TYPE_UNKNOWN` (0) |
| `Software` | null |
| `Flags` | the expression above |
| `pFeatureLevels` | `{11_1}` first, then the six-level list |
| `FeatureLevels` | 1, then 6 |
| `SDKVersion` | 7 (`D3D11_SDK_VERSION`) |
| `ppDevice` | the provider's device slot |
| `pFeatureLevel` | the provider's level slot |
| `ppImmediateContext` | the provider's context slot |

**Negative:** the client's own path does not use
`D3D11CreateDeviceAndSwapChain`. The only reference to that string
(`0x1435e2530`) is in the AMD AGS wrapper
`agsDriverExtensionsDX11_CreateDevice` (`0x142c0a146`); `pm_dx11` creates the
device with `D3D11CreateDevice` and the swap chain separately through
`pm_dxgi`.

**The two probe call sites.** `FUN_14265ea70` (`0x14265ea70`, reached from
`FUN_14265ec80`) and `FUN_14267e5f0` (`0x14267e5f0`, the hardware-info path)
each create a **temporary** device with the adapter, `D3D_DRIVER_TYPE_UNKNOWN`,
no software module, **flags 0**, no feature-level array, SDK 7, and then release
it. `FUN_14265ea70` queries `ID3D11Device::CheckFeatureSupport` with feature
`0xe` into a 0x20-byte block; `FUN_14267e5f0` calls `FUN_14267cd70`
(`0x14267cd70`), which walks the device's counter list looking for the Intel
device-information counter. Neither is the renderer's device; their results
feed the hardware/vendor information structure, not a swap chain.

**Vendor wrapper selection** (`FUN_142661a90`, `0x142661a90`): the provider
object at device-create `+0x60` points at one of four extension objects. The
vendor id selects AMD (`0x1002`, object at provider `+0x70`), NVIDIA
(`0x10de`, object at provider `+0xb0`), Intel (`0x8086`, object at provider
`+0x90`), or the generic object at provider `+0x68`. The AMD/Intel/generic
vtables are `0x143496c78`; the NVIDIA vtable is `0x143496cf0`. All four route
their create slot (`+8`) to `0x14267d270`. The extension may substitute its own
device-create entry; the wrapper still receives the same argument list.

**Adapter selection.** `pm_dxgi::Instance::InitGpus` (`0x14262f0e9`) enumerates
adapters with `IDXGIFactory1::EnumAdapters1` from index 0 until
`DXGI_ERROR_NOT_FOUND`, records each adapter's description, flags the Microsoft
Basic Render Driver, and sorts the list (`FUN_14262ba20`, `0x14262ba20`). The
sort's preference order is **open**. `pm_api::PlatformInstance::CreateDevice`
then walks `platform+0x3a0` (`platform+0x3a8` count) in order; if the request
names a GPU at `+8`, only that GPU is tried. D1 takes the first enumerated
adapter, which is the same order on a single-GPU machine; a multi-GPU
preference is **open**.

**Debug name and frame latency.** `pm_dx11::Device::Init` sets the debug name
`0x143497c48` on the device and calls the DXGI frame-latency setter with
`device descriptor +0x20`; the failure string is `SetMaximumFrameLatency
Failed` (`0x143497...`). The latency value's producer is **open**.

**Deferred context.** When instance/device flag `+0x450` is non-zero,
`pm_dx11::Device::Init` creates a deferred context (`CreateDeferredContext`;
failure string `0x143497...`) and a 1 MB ring thread
(`FUN_14269d680` with `0x100000`); otherwise the immediate context is used.
Which production setting selects the mode is **open**.

### D3D11 swap chain

The swap chain is created by the shared `pm_dxgi::Instance::InitSwapChain`
(`0x14262f9d0`, log `0x14348c3c0`). **There is no D3D11/D3D12 branch in this
function**; it is backend-neutral and receives the device as an argument. The
D3D11-specific work is the back-buffer resource creation in
`pm_dx11::Device::CreateSwapChainResources` (`0x14266a060`). `InitSwapChain`
chooses one of two *factory* paths:

- **new path** (instance `+0x78` non-zero): `CreateSwapChainForHwnd` (factory
  vtable `+0x78`) with a `DXGI_SWAP_CHAIN_DESC1`; failure string
  `CreateSwapChainForHwnd Failed` (`0x14348c3a0`);
- **old path** (instance `+0x78` zero): `CreateSwapChain` (factory vtable
  `+0x50`) with `DXGI_SWAP_CHAIN_DESC`; failure string `CreateSwapChain
  Failed` (`0x14348c408`).

Both take the device from the call's `param_5` and the window handle from
request `+0x00`. On this machine the factory supports `IDXGIFactory2` (the D1
smoke run used the new path); the old path is not implemented.

The `DXGI_SWAP_CHAIN_DESC1` fields, read from the new path:

| `DESC1` field | source | default view value |
|---|---|---|
| `Width` | request `+0x20` | window client width |
| `Height` | request `+0x24` | window client height |
| `Format` | request `+0x2c` through the table `0x14348bf10`, special cases `0x20 -> 28`, `0x25 -> 87` | index `0x1f` → **28** (`R8G8B8A8_UNORM`) |
| `Stereo` | 0 | `FALSE` |
| `SampleDesc` | count 1, quality 0 | `1 / 0` |
| `BufferUsage` | `((+0x40 & 1) + 2) << 4`, plus `0x400` when `+0x40` bit 1 | **`0x30`** = RENDER_TARGET_OUTPUT (`0x20`) + SHADER_INPUT (`0x10`) |
| `BufferCount` | request `+0x30` | **3** |
| `Scaling` | 0, or the table at `0x14348c150` indexed by request `+0x44` on the flip path | **0** (`DXGI_SCALING_STRETCH`) |
| `SwapEffect` | 0, or 4 on the flip path (`instance+0xb0 != 0` and the factory-QI flag) | **0** |
| `AlphaMode` | 0 | `DXGI_ALPHA_MODE_UNSPECIFIED` |
| `Flags` | `(+0x28 & 1 ? 0x2 : 0)`, plus `0x40` on the tearing path, plus `0x800` when `instance+0xb0` and the flip path | **0** |

`0x2` is `DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH`; `0x40` is
`DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`. The `0x800` member is
**unnamed** by the read code; the value is pinned, the enum name is open. The
swap-effect enumeration behind the two read values `0` and `4` is not claimed
by the client's code; `0` is the default path and `4` the flip branch. The old
`DXGI_SWAP_CHAIN_DESC` path stores `0` or `2` in its swap-effect field from
request `+0x28` bit 0, which is the mode-switch flag in the new path; the two
paths are not equivalent and only the new path is implemented.

The default request is built by the engine view create `FUN_141ef6960`
(`0x141ef6960`):

- `view+0x40 = width`, `view+0x44 = height`;
- `view+0x50 = 3` (buffer count);
- `view+0x4c = 0x1f` (format index) unless `renderer+0x48b8a` is set; the
  alternative producer `FUN_142636500` (`0x142636500`) is **open**;
- `view+0x60 |= 1` (usage bit 0);
- `view+0x64 = 2`;
- `view+0x54` is a boolean from a runtime config global (the decompiler
  reaches it through an obfuscated pointer expression; the default is **not**
  read);
- the request is at `view+0x20` and is passed through `FUN_14264acd0`
  (`0x14264acd0`) → `FUN_14265d5f0` (`0x14265d5f0`) → `InitSwapChain`.

**Sync interval.** `FUN_14265e620` (`0x14265e620`) copies the view's 15-qword
request and calls `UpdateSwapChain` (`0x1426320f0`). `UpdateSwapChain` writes
request `+0x34` into the present object's `+0x60`; on the view, request `+0x34`
is `view+0x54`, the config boolean. `Present` (`0x142630790`) reads present
`+0x60` as the sync interval. So the chain is read: **view+0x54 (bool) →
request+0x34 → present+0x60 → `Present`'s first argument**. The value is 0 or 1;
which one the shipped config produces is **open**. D1 exposes it as a parameter
and the runnable path passes 1 so the present is paced; that is a run choice,
not a client claim.

**Present flags.** `Present` passes `0`, or `0x200`
(`DXGI_PRESENT_ALLOW_TEARING`) when all of instance `+0xb0 != 0`, present
`+0x60 == 0`, present `+0x48 == 0`, present `+0x49 != 0` hold. The default view
does not set the tearing path.

**Resize.** `pm_dxgi::Instance::UpdateSwapChain` (`0x1426320f0`) compares the
requested width/height/format with the current description, calls
`FindClosestMatchingMode`, `SetFullscreenState`, `ResizeTarget` and
`ResizeBuffers(count, width, height, format, flags)`, with count = request
`+0x30` (3) and format = the resolved request format. The windowed client adds
a resize with the same count and format; the fullscreen/target-mode part of
`UpdateSwapChain` is not D1 behaviour and is **open**.

### D3D11 swap-chain resources (D2)

`pm_dx11::Device::CreateSwapChainResources` (`0x14266a060`, log string
`0x143498490`) is the back-buffer side. The operation order is read exactly:

1. `IDXGISwapChain::GetBuffer(0, IID_ID3D11Texture2D)` (swap-chain vtable
   `+0x48`, IID `0x14331e8f0`); failure string `GetBuffer Failed`
   (`0x143497cf0`).
2. Resolve the engine format byte (`request+0x2c`) to the **view** format:
   the six explicit special cases first, then the 64-dword table at
   `0x1434965d0`. There is no range check; a byte `>= 64` reads past the
   table and no shipped swap-chain view produces one.
3. `ID3D11Device::CreateRenderTargetView` (device vtable `+0x48`) with an
   **explicit** descriptor: `Format` = the resolved view format,
   `ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D` (4), `MipSlice = 0`.
   Failure string `CreateRenderTargetView Failed` (`0x143497f40`).
4. Publish the RTV into the resource object (`FUN_142673600`),
   `ID3D11Texture2D::GetDesc` (texture vtable `+0x50`) for the reported size,
   then record the width/height and the packed format/usage bytes.
5. If engine usage bit 0 (`request+0x40 & 1`) is set, create the
   shader-resource view with `Format` = the resolved view format,
   `ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D` (4), `MostDetailedMip = 0`,
   `MipLevels = 1`; failure string `CreateShaderResourceView Failed`
   (`0x143497f88`).
6. If engine usage bit 1 is set, create the unordered-access view with
   `Format` = the resolved view format,
   `ViewDimension = D3D11_UAV_DIMENSION_TEXTURE2D` (4), `MipSlice = 0`;
   failure string `CreateUnorderedAccessView Failed` (`0x143497cf0` area).

The SRV is created **before** the UAV; the default view sets usage bit 0 only,
so it gets an RTV and an SRV and no UAV. The descriptor's mip slice, mip
level, most-detailed mip and dimension are all explicit in the client's code;
nothing is inferred from the texture.

**The format tables.** The view table at `0x1434965d0` and the swap-chain
buffer table at `0x14348bf10` are **byte-identical** (both read, all 64
dwords). The buffer path adds two special cases of its own: `0x20 -> 28` and
`0x25 -> 87`, so engine byte `0x20` is an **UNORM buffer (28) with an sRGB
view (29)**. The view table is:

| index | value | index | value | index | value | index | value |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 16 | 52 | 32 | 29 | 48 | 12 |
| 1 | 61 | 17 | 0 | 33 | 31 | 49 | 14 |
| 2 | 0 | 18 | 115 | 34 | 30 | 50 | 10 |
| 3 | 63 | 19 | 0 | 35 | 32 | 51 | 7 |
| 4 | 62 | 20 | 85 | 36 | 87 | 52 | 8 |
| 5 | 64 | 21 | 0 | 37 | 91 | 53 | 6 |
| 6 | 65 | 22 | 86 | 38 | 24 | 54 | 3 |
| 7 | 56 | 23 | 42 | 39 | 25 | 55 | 4 |
| 8 | 58 | 24 | 43 | 40 | 0 | 56 | 2 |
| 9 | 57 | 25 | 41 | 41 | 26 | 57 | 71 |
| 10 | 59 | 26 | 35 | 42 | 67 | 58 | 72 |
| 11 | 54 | 27 | 37 | 43 | 17 | 59 | 74 |
| 12 | 49 | 28 | 36 | 44 | 18 | 60 | 75 |
| 13 | 0 | 29 | 38 | 45 | 16 | 61 | 77 |
| 14 | 51 | 30 | 34 | 46 | 11 | 62 | 78 |
| 15 | 50 | 31 | **28** | 47 | 13 | 63 | 80 |

Special cases: `0x83 -> 0x38`, `0x84 -> 0x29`, `0x85/0x86 -> 0x2e`,
`0x87 -> 0x15`, `0x88 -> 0x3d`. Index `0x1f` (the default view) resolves to
**28**, the same as the buffer.

**Resize order.** `UpdateSwapChain` (`0x1426320f0`) compares the requested
size/format with `GetDesc`, optionally calls `FindClosestMatchingMode`,
`SetFullscreenState` and `ResizeTarget`, then invokes two callbacks passed by
`FUN_142679780`:

1. release (`LAB_14265ff70` → `FUN_14266bd90`): texture, RTV, SRV, UAV, each
   only when present;
2. `ResizeBuffers(count = request+0x30, width, height, format, flags = 0)`;
3. recreate (`FUN_14265ff80` → `CreateSwapChainResources`), i.e. GetBuffer →
   RTV → optional SRV → optional UAV.

`ow-render` forms the same `ResizeBuffers` call and relies on the
`SwapResources` field order (texture, RTV, SRV, UAV) as the release order; the
recreate is the same `create_swap_resources` call. The fullscreen/target-mode
branch of `UpdateSwapChain` is **not** D2 behaviour and is open.

**D2 status.** The view descriptor values, the usage gates, the two tables and
the resize order are pinned by `crates/ow-render` tests. The rendered result is
**not compared against the client**: a client capture is impossible under the
anti-cheat.

### D3D11 frame loop and command replay

The frame begin/end slots are backend-neutral:

| slot | entry | function |
|---|---|---|
| 129 | `0x1433585d0` → `0x141ef0600` | frame begin |
| 130 | `0x1433585d8` → `0x141ef9c80` | GPU wait / frame pacing |
| 131 | `0x1433585e0` → `0x141ef0800` | frame end and present |

`FUN_141ef0600` waits on the renderer's GPU state, reconfigure the swap chain
when the view changed, releases deferred targets, and handles the three frame
slots. `FUN_141ef0800` walks the view list, runs the final composite/blit, emits
the two per-frame batches, advances the frame counter, presents each view
through `FUN_14265e8f0` (`0x14265e8f0`) → `pm_dxgi::Instance::Present`, and
marks the GPU phase end. `FUN_141f0caa0` (`0x141f0caa0`) is the per-view submit:
it appends command lists, transitions the batch resources, sets the submitted
list and waits on a fence.

The D3D11 queue:

| operation | address | evidence |
|---|---|---|
| `pm_dx11::CmdQueueImpl::Init` | `0x142673710` | log `0x1434985a8`; creates a device query (`ID3D11Device` vtable `+0xc0`) |
| `pm_dx11::CmdQueueImpl::Submit` | `0x142677dc0` (wrapper `0x142677da0`) | log `0x1434985f8` at `0x142677e79`; immediate path calls the replay under device lock `+0x30`; deferred path calls `FinishCommandList` |
| replay / `ExecuteReplayList` | `FUN_14266c690` `0x14266c690` | log `0x1434983a0`; Ghidra's stored body is degenerate, the dispatch below is read from the entry |
| missing opcode | `0x1434983d0` | log `D3D11: missing op code: %u` |

### D3D11 frame order (D3)

The per-frame order below is the read call sequence. `crates/ow-render::frame`
holds it as `Step::ORDER`; a unit test pins it, and the D3D11 runnable logs one
`renderer: D3: <step>` line per step.

| # | step | read client function(s) | what is read |
|---|---|---|---|
| 1 | `FrameBegin` | `0x141ef0600` (slot 129) | pending-frame submit, `FUN_14265e620` swap-chain update, three frame-slot releases (`FUN_142654e90` + `FUN_141f0edd0` → `FUN_142655020`/`FUN_14265a710`), descriptor recycle (`FUN_141f9fa00`) |
| 2 | `GpuWait` | `0x141ef9c80` (slot 130) | marker/pacing tick, deferred-slot release, engine device slots `+0x3d8`/`+0x40` |
| 3 | `ScenePrepare` | `0x141ef8140` | per-frame buffer update (`FUN_141ef2260`, `FUN_141f0f230`), record `0x30` for the three frame lists, batch-resource transitions (`FUN_142639020`), per-view resource prep (`FUN_141f10540`, `FUN_142638bb0`, `FUN_141f02d20`) |
| 4 | `UploadFlush` | `0x142650340` → `0x142650430` | queue **Finish**: flush, signal fence, `SetEventOnCompletion`, `WaitForSingleObject(INFINITE)` |
| 5 | `CommandReplay` | `0x142677dc0` → `0x14266c690` | queue dispatch to the backend submit; immediate path replays the record stream under device lock `+0x30`; deferred path `FinishCommandList` |
| 6 | `FenceWait` | `0x14265adf0` | pm fence object: completed-value probe, `SetEventOnCompletion`, `WaitForSingleObject(timeout / 1e6)` |
| 7 | `PerViewSubmit` | `0x141f0caa0` | record `0x30` emission, queue flush (`FUN_142650800`), fence wait, slot release, frame-list reset |
| 8 | `FrameEnd` | `0x141ef0800` (slot 131) | final composite (`FUN_141df2280`) and blit (`0x141f97240`) records, record `0x4a`/`0x1be`, batch records `0x387`, per-view submit, markers 5/6, `FUN_14265e8f0` present |
| 9 | `Present` | `0x14265e8f0` → `0x142630790` | queue flush, then `IDXGISwapChain::Present(sync interval, flags)` |

**Slot 132.** The shared frame table has a fourth entry at `0x1433585e8` →
`FUN_141efa380`, directly after slot 131. It advances the frame index
(`renderer+0x48260 = counter % 3`) and flushes batches A and B. The outer call
order of slots 129–132 is not read, so `Step::ORDER` does not include it yet.

**The submit path.** The neutral queue's flush is `FUN_142652600`; it calls
`FUN_14265a100` → `FUN_1426500d0` → `FUN_14263a710`, and `FUN_14263a710`
dispatches through the queue object's vtable **`+0x50`**. That slot is
`pm_dx11::CmdQueueImpl::Submit` (`0x142677da0` → `0x142677dc0`) in the three
D3D11 queue vtables at `0x143497a20`, `0x143497a98` and `0x143497b10` (created
by `FUN_142667250`, called from `pm_dx11::Device::Init`). The immediate path
calls the replay `FUN_14266c690`; the deferred path uses
`FinishCommandList`.

**The upload finish.** `ScenePrepare` ends in `FUN_142650340` (a lock wrapper
around `FUN_142650430`), which runs the flush above, signals the queue's fence
to the next value, sets the queue event and waits for it with an infinite
timeout (`WaitForSingleObject`). So the constant/batch upload is submitted and
completed before the frame's main command list is built.

**The frame end and present.** `FUN_141ef0800` runs the final composite/blit
per view, emits record `0x4a`, the conditional `0x1be`, the two `0x387` batch
records, then the per-view submit (`FUN_141f0c950` / `FUN_141f04f40`) and
presents each view through `FUN_14265e8f0`: that wrapper flushes the queue
(`FUN_142652600`) and calls `pm_dxgi::Instance::Present` (`0x142630790`),
which selects the swap-chain interface from present `+0x30` / the object and
calls `IDXGISwapChain::Present(sync interval, flags)`. The flags are `0`, or
`DXGI_PRESENT_ALLOW_TEARING` (`0x200`) only when all four read conditions
hold: instance `+0xb0` non-zero, sync interval zero, present `+0x48` zero and
present `+0x49` non-zero. The default view takes neither the tearing path nor
the waitable-object flag, so the flag word is `0`.

**What D3 does not read.**

- The **outer caller** that invokes the three shared frame slots is not
  identified. Steps 1 and 2 are adjacent in the interface vtable (129 then
  130) and in that order in `Step::ORDER`, but the call order between them is
  not proven by a read call site.
- The **context reset/clear/bind** step is read through D5. `0x0c` is the
  render-target bind (`OMSetRenderTargets` at `0x14266e54d`) after the per-RTV
  clears, the DSV clear and the pm releases; `0x0d` is the MSAA resolve plus
  unbind (`OMSetRenderTargets(0,NULL,NULL)` at `0x14266e6ba`).
  `FUN_142674980` is a separate per-draw lazy flush of the UAV/CB/SRV state
  caches, called at the start of `0x25`/`0x26` and by `0x27`/`0x28`. The engine
  pass order that emits `0x0c`/`0x0d` is not read.
- The **D3D11 fence** body is a pm object whose D3D11 implementation is not
  read. The `pm_dx11::CmdQueueImpl::Init` leaf creates an
  `ID3D11Query` (type dword `0`) and calls `Begin` on the immediate context,
  but the wait/`GetData` side is not read.
- The pm queue's flush body is read, but **no read command stream exists in
  this crate**. The D4 executor is implemented and synthetically tested, but
  the frame step has no stream to replay; the runnable path therefore executes
  only `Present` and logs every other step as open. The D1 placeholder clear
  is a separate, labelled line, not a D3 step.

**D3 status.** The step order, the flush/finish/submit chain, the per-view
submit and the present values are pinned by `crates/ow-render` tests. The GPU
call order is **not compared against the client**; a client capture is
impossible under the anti-cheat.

### D3D11 command replay: dispatch and read handlers (D4/D5)

**Record layout.** A record is a 0x18-byte header followed by its payload.
All integers are little-endian.

| offset | width | meaning |
|---|---|---|
| `+0x00` | 8 | segment link, read only when the read offset crosses the segment capacity |
| `+0x08` | 4 | segment capacity |
| `+0x10` | 4 | opcode |
| `+0x14` | 4 | record byte size, **including the header** |
| `+0x18` | .. | payload |

**Dispatch.** The replay entry is `FUN_14266c690` (`0x14266c690`), reached
from `pm_dx11::CmdQueueImpl::Submit` (`0x142677dc0`) on the immediate path.
The read offset is the command-list object's `+0x20`, the data pointer is
`+0x18` and the segment capacity is `+0x8`. The offset is advanced by the
record size **before** dispatch. An opcode above `0x3f` calls the missing
handler at `0x142670fe2` and logs `D3D11: missing op code: %u`
(`0x1434983d0`); it does not abort. Opcodes `0..0x3f` jump through the
64-entry table at **`0x142671154`**. Opcode `0x00` is the end/flush record: it
runs nine stage-cache flush helpers and returns from the replay. Opcode `0x1d`
points at the loop head (`0x14266c736`) and is a no-op.

**Correction to the earlier partial read.** The first pass named six opcodes
from their call-site offsets. The full handler read corrects them:
`0x09` is `ClearUnorderedAccessViewFloat`, `0x25` is `Draw`/`DrawInstanced`,
`0x26` is `DrawIndexed`/`DrawIndexedInstanced`, `0x2d` is a pm state-cache
`+0x48` call, and **`0x04` is not `ClearDepthStencilView`**: its handler calls
`context+0x70` (`Map`) and `context+0x78` (`Unmap`) after helpers
`0x14265b2c0`/`0x141f122f0`/`0x142669b10`, with a resource handle at `+0x18`.
`ClearDepthStencilView` (`+0x1a8`) **is** in the replay: `0x08` calls it
directly with the view at `+0x18`, clear flags at `+0x20`, depth at `+0x24`
and the stencil byte at `+0x28`; `0x0c` also calls it per render-target entry,
through an out-of-band descriptor. The earlier "no depth clear" statement was
wrong. The colour clear is `0x07`, the viewport (`0x29`) and `Dispatch`
(`0x30`) names were correct. The colour clear is `vtable+0x190`, **not**
`+0x188` (`+0x188` is `CopyStructureCount`). `OMSetRenderTargets` is
`vtable+0x108`; `0x0c` binds at `0x14266e54d`, `0x0d` unbinds at
`0x14266e6ba`.

**The 64-opcode table.** `impl` means the executor calls the named sink
method with the payload arguments. `open` means the handler was read only up
to the stated gap; the executor logs it and never fakes the call. A 16-byte
payload block is four `f32` (colour/blend), four `u32` (UAV clear), or the
mixed clear block `{flags u32, depth f32, stencil u32, pad}` (`0x08`); an
8-byte value is a client pointer/handle. The executor treats it as an opaque
key and never dereferences it; the D3D11 sink resolves it through
`ResourceTable`.

| op | target | read handler / payload | state |
|---|---|---|---|
| `0x00` | `0x14267102d` | end/flush: nine stage-cache flush helpers, then return | impl: end-of-stream (no deferred binds) |
| `0x01` | `0x14266c805` | resource handle `+0x18`, qword `+0x20`; `SyncBufferForUsage`; context calls at `+0x18`, `+0x170`, `+0x70`, `+0x78`, `+0x10` | open: resource table and map/copy path |
| `0x02` | `0x14266ca72` | resource descriptor `+0x18/+0x20/+0x30/+0x40/+0x50`; format-size helpers `0x14265b2c0`/`0x141f122f0` | open: texture update, resource table |
| `0x03` | `0x14266cc1a` | `UpdateSubresource` (`+0x180`); source data and box are out-of-band | open |
| `0x04` | `0x14266ce03` | `Map` (`+0x70`) / `Unmap` (`+0x78`): resource handle `+0x18`, fields `+0x20/+0x30/+0x40`, helpers `0x14265b2c0`/`0x141f122f0`/`0x142669b10` | open: resource table and map/unmap path |
| `0x05` | `0x14266d0f4` | `CopyResource` (`+0x178`) after two `SyncBufferForUsage` lookups (`+0x18`/`+0x20`) | open: resource table |
| `0x06` | `0x14266d16b` | `CopyResource` (`+0x178`) on pm pointers `+0x18`/`+0x20` (deref `+0x8`) | open: resource table |
| `0x07` | `0x14266d1ab` | `ClearRenderTargetView` (`+0x190`): view `+0x18`, colour `+0x20..+0x2f` | **impl** |
| `0x08` | `0x14266d1ed` | `ClearDepthStencilView` (`+0x1a8`): view `+0x18`, flags `+0x20`, depth `+0x24`, stencil byte `+0x28` | **impl** |
| `0x09` | `0x14266d22d` | `ClearUnorderedAccessViewFloat` (`+0x1a0`): view `+0x18`, values `+0x20`; `SyncBufferForUsage` `+0x30` | open: sync path |
| `0x0a` | `0x14266d298` | `ClearUnorderedAccessViewUint` (`+0x198`): view `+0x18`, values `+0x20`; `SyncBufferForUsage` `+0x30` | open: sync path |
| `0x0b` | `0x14266d303` | `ResolveSubresource` (`+0x1c8`): dst `+0x18`, src `+0x20`, format table `0x1434965d0` | open: resource handles |
| `0x0c` | `0x14266d36b` | render-target set: count `+0x18`, RTV array `+0x20` (0x30-byte entries: pm view `+0x00`, native RTV through `[view+0x10]`, action `+0x10`, colour `+0x20`), DSV block `+0x28` (pm view `+0x00`, native DSV through `[view+0x20]`, depth action `+0x10`, depth `+0x20`, stencil action `+0x28`, stencil `+0x38`), state id `+0x30`; per entry clear/release, then `OMSetRenderTargets(count, native RTV array, DSV)` at `0x14266e54d` | read: arrays out-of-band; alias-unbind, cache model and `apply_render_targets` in `ow-render::state`; **implemented in the executor** |
| `0x0d` | `0x14266e55c` | pending-target apply: `ResolveSubresource` (`+0x1c8`) per MSAA entry, pm release `+0x3b0`, then `OMSetRenderTargets(0,NULL,NULL)` (`+0x108`) at `0x14266e6ba` | **implemented** |
| `0x0e` | `0x14266e6d3` | graphics reset: zero five CB counters and six resource counters, UAV flush `0x1426790e0` (`+0x110`), copy 0x58-byte state block at `[+0x18]+0x28` | open: state block out-of-band |
| `0x0f` | `0x14266e7a7` | state block at `[+0x18]`: `IASetInputLayout` `+0x88`, `IASetPrimitiveTopology` `+0xc0`, `VSSetShader` `+0x58`, `PSSetShader` `+0x48`, `DSSetShader` `+0x200`, `HSSetShader` `+0x1e0`, `GSSetShader` `+0xb8`, `OMSetDepthStencilState` `+0x120`, `OMSetBlendState` `+0x118`, `RSSetState` `+0x158`, pm blend `+0x48`, 0x58-byte block at state-block `+0x60` | open: state block out-of-band |
| `0x10` | `0x14266e9da` | VS constant buffers: start slot `+0x1c`, count `+0x20`, 0x20-byte array `+0x28`; `0x142676130` + `SyncBufferForUsage`; end flush `0x142660620` stage 0 (`VSSetConstantBuffers` `+0x38` / pm ext `+0x3b8`) | open: array/resource objects out-of-band |
| `0x11` | `0x14266ea13` | HS constant buffers: same layout; end flush stage 1 (`HSSetConstantBuffers` `+0x1f0` / pm ext `+0x3c0`) | open |
| `0x12` | `0x14266ea4c` | DS constant buffers: same layout; end flush stage 2 (`DSSetConstantBuffers` `+0x210` / pm ext `+0x3c8`) | open |
| `0x13` | `0x14266ea85` | GS constant buffers: same layout; end flush stage 3 (`GSSetConstantBuffers` `+0xb0` / pm ext `+0x3d0`) | open |
| `0x14` | `0x14266eabe` | PS constant buffers: same layout; end flush stage 6 (`PSSetConstantBuffers` `+0x80` / pm ext `+0x3d8`) | open |
| `0x15` | `0x14266eaf7` | CS constant buffers: same layout; end flush stage 7 (`CSSetConstantBuffers` `+0x238` / pm ext `+0x3e0`) | open |
| `0x16` | `0x14266eb30` | VS shader resources: start slot `+0x1c`, count `+0x20`, 0x20-byte array `+0x28`; `SyncBufferForUsage` per element; flush `0x142678bf0` -> `VSSetShaderResources` `+0xc8` | open: array/resource objects out-of-band |
| `0x17` | `0x14266ec66` | HS shader resources: same layout; flush `0x142678570` -> `HSSetShaderResources` `+0x1d8` | open |
| `0x18` | `0x14266eda6` | DS shader resources: same layout; flush `0x142678710` -> `DSSetShaderResources` `+0x1f8` | open |
| `0x19` | `0x14266eee6` | GS shader resources: same layout; flush `0x142678d90` -> `GSSetShaderResources` `+0xf8` | open |
| `0x1a` | `0x14266f026` | PS shader resources: same layout; flush `0x142678a50` -> `PSSetShaderResources` `+0x40` | open |
| `0x1b` | `0x14266f166` | CS shader resources: same layout; flush `0x1426788b0` -> `CSSetShaderResources` `+0x218` | open |
| `0x1c` | `0x14266f2a6` | table-dispatched function selected by a payload byte | open: table target |
| `0x1d` | `0x14266c736` | loop head | **impl**: no-op |
| `0x1e` | `0x14266f323` | same shader-resource layout | open |
| `0x1f` | `0x14266f466` | same shader-resource layout | open |
| `0x20` | `0x14266f5a6` | constant upload `0x1426798c0` + function pointer; payload `+0x10/+0x20` | open: upload cadence/target |
| `0x21` | `0x14266f659` | resource lookup + object vtable slot 0 | open: object type |
| `0x22` | `0x14266f71a` | resource lookup + `0x1426608a0` + function pointer | open: cache update |
| `0x23` | `0x14266f7dd` | same as `0x22` | open |
| `0x24` | `0x14266f919` | stage-cache apply: `SyncBufferForUsage` + stage setters | open: deferred cache |
| `0x25` | `0x1426701c9` | `Draw` (`+0x68`) if instance `1`/start `0`, else `DrawInstanced` (`+0xa8`): vertex `+0x18`, instance `+0x1c`, start vertex `+0x20`, start instance `+0x24` | **impl** |
| `0x26` | `0x142670267` | `DrawIndexed` (`+0x60`) if instance `1`/start `0`, else `DrawIndexedInstanced` (`+0xa0`): index `+0x18`, instance `+0x1c`, start index `+0x20`, base vertex `+0x24`, start instance `+0x28` | **impl** |
| `0x27` | `0x142670320` | two `SyncBufferForUsage` + pm cache `+0x50`; descriptor `+0x20` | open: cache method |
| `0x28` | `0x142670409` | two `SyncBufferForUsage` + pm cache `+0x58`; descriptor `+0x20` | open: cache method |
| `0x29` | `0x1426704b7` | `RSSetViewports` (`+0x160`), one viewport: x `+0x18`, y `+0x1c`, width `+0x20`, height `+0x24`, min `+0x28`, max `+0x2c` | **impl** |
| `0x2a` | `0x142670538` | `RSSetScissorRects` (`+0x168`), one rect: left `+0x18`, top `+0x1c`, width `+0x20`, height `+0x24`; right/bottom are left+width/top+height | **impl** |
| `0x2b` | `0x1426705a8` | `OMSetDepthStencilState` (`+0x120`): cached state, stencil ref `+0x18` | open: state cache |
| `0x2c` | `0x1426705dc` | `OMSetBlendState` (`+0x118`): cached state, blend factor `+0x10`, sample mask `+0x20` | open: state cache |
| `0x2d` | `0x142670639` | pm object at queue `+0x38`, `+0x48` call (call site `0x142670667`); 16-byte payload `+0x10` shuffled to float args plus a byte arg; same setter shape as `0x0f`'s `device+0xa0` blend call (`0x14266e91f`); ruled out as the RT-bind trigger | open: cache method |
| `0x2e` | `0x14267067b` | flush PS SRV cache (`0x142678f30`) | open: deferred cache |
| `0x2f` | `0x1426706e5` | `CSSetShader` (`+0x228`, call site `0x1426706fb`): state-object pointer `+0x18`; shader at state `+0x8`, class instances NULL, count 0 | open: state object out-of-band |
| `0x30` | `0x14267072a` | `Dispatch` (`+0x148`): x `+0x18`, y `+0x1c`, z `+0x20` | **impl** |
| `0x31` | `0x142670789` | `DispatchIndirect` (`+0x150`): buffer `+0x18` (`SyncBufferForUsage`), offset `+0x20` | open: resource table |
| `0x32` | `0x142670803` | create SRV + `GenerateMips` (`+0x1b0`) | open: SRV creation |
| `0x33` | `0x1426709ae` | annotation `BeginEvent`, string `+0x10` | open: annotation |
| `0x34` | `0x142670a25` | annotation `EndEvent` | open: annotation |
| `0x35` | `0x142670a53` | annotation `SetMarker`, string `+0x10` | open: annotation |
| `0x36` | `0x142670a9d` | `Begin(query)` (`+0xd8`) from array `+0x18` | open: query array |
| `0x37` | `0x142670af9` | `End(query)` (`+0xe0`) from array `+0x18` | open: query array |
| `0x38` | `0x142670b49` | append query result (`0x142679ff0`/`0x142388210`/memcpy) | open: list state |
| `0x39` | `0x142670c3a` | `Begin`+`End` on two queries from array `+0x10` | open: query array |
| `0x3a` | `0x142670c9c` | `End` on two queries from array `+0x10` | open: query array |
| `0x3b` | `0x142670d02` | grow query list (`0x1424c45a0`) | open: list state |
| `0x3c` | `0x142670da3` | pm method `+0x30`, payload `+0x18/+0x20` | open: pm object |
| `0x3d` | `0x142670e14` | pm method `+0x30`, payload `+0x10/+0x20` | open: pm object |
| `0x3e` | `0x142670ed8` | pm method `+0x38`, payload `+0x18` | open: pm object |
| `0x3f` | `0x142670f3b` | pm method `+0x38` in a loop | open: pm object |

**Implemented set.** The executor implements `0x00`, `0x07`, `0x08`, `0x0c`,
`0x0d`, `0x1d`, `0x25`, `0x26`, `0x29`, `0x2a` and `0x30`. The nine context
handlers are `ClearRenderTargetView`, `ClearDepthStencilView`, the
render-target set, the pending resolve/unbind, `Draw`/`DrawInstanced`,
`DrawIndexed`/`DrawIndexedInstanced`, `RSSetViewports`, `RSSetScissorRects` and
`Dispatch`. The other 53 opcodes are open by opcode in
`crates/ow-render::replay`; the executor calls `Sink::open` and never an
operation method.

**Render-target bind: opcode `0x0c`.** The handler is
`0x14266d36b..0x14266e55c`. It reads the count at `+0x18`, the RTV array at
`+0x20`, the DSV block at `+0x28` and the state id at `+0x30`. For each
`0x30`-byte RTV entry it reads the pm view at `+0x00`, the native RTV through
`[view+0x10]`, the action byte at `+0x10` and the colour at `+0x20`; action
`2` clears through `ClearRenderTargetView` (`+0x190`) at `0x14266d417`, action
`0` releases through pm `+0x3b0` at `0x14266d400`. For the DSV block it reads
the native DSV through `[view+0x20]`, the depth action at `+0x10`, depth at
`+0x20`, stencil action at `+0x28` and stencil at `+0x38`; it builds
`flags = (depth_action == 2 ? 1 : 0) | (stencil_action == 2 ? 2 : 0)` and
calls `ClearDepthStencilView` (`+0x1a8`) at `0x14266d4e8` when the flags are
non-zero. It then copies the native RTV pointers into the replay's array and
calls `OMSetRenderTargets(count, array, dsv)` at `0x14266e54d`. Opcode `0x0d`
resolves the pending MSAA entries and calls `OMSetRenderTargets(0,NULL,NULL)`
at `0x14266e6ba`.

**`0x0c` alias-unbind sub-path.** Between the clears and the bind, `0x0c` runs
six unbind groups: a CS UAV bulk unbind at `0x14266d658` (`+0x220`), a CS SRV
bulk unbind at `0x14266d790` (`+0x218` via `0x1426637d4`), and per-RTV
VS/PS/HS/DS/GS SRV single-slot unbinds at `0x14266d8db` (`+0xc8`), `0x14266da2b`
(`+0x40`), `0x14266db7b` (`+0x1d8`), `0x14266dccb` (`+0x1f8`) and
`0x14266de1b` (`+0xf8`), then the same five stages against the DSV at
`0x14266df8a`, `0x14266e0db`, `0x14266e23a`, `0x14266e38b` and `0x14266e4ea`.
Every per-slot call passes `NumViews=1` and a pointer to a zero qword, so it
binds nothing; it removes an SRV that aliases the current RTV/DSV. The alias
test `FUN_14265fb90` compares key `+0x08`, the mip/array fields in `+0x0c`
(bits 0..3, 4..7, 8..18, 19..29), and the underlying resources through view
vtable `+0x38`. The sub-path consumes the replay's per-stage SRV caches at
`0x29a0`, `0x49d0`, `0x6a00`, `0x8a30`, `0xaa60`, the CS SRV cache at
`0xca90`, the CS UAV cache at `0xeef0`, and their start/count/dedup/bitmask
state. That state is populated by opcodes `0x16..0x1b` and the UAV path; it
cannot be rebuilt from the `0x0c` record plus a simple resource table. This is
the remaining `0x0c` gap.

**`0x0d` resolve and release.** The loop is `ESI < count`, entry
`RBP+0x158+ESI*0x30`; it skips an entry whose view or native view is null. The
entry action at `+0x11` selects resolve (`2`/`3`), release-only (`0`) or no-op
(`1`). The resolve at `0x14266e5d2` passes the destination `[entry+0x18]`
(native `+0x08`, subresource `+0x34`, format index `+0x30`) and the source
`[entry+0x00]` (native `+0x08`, subresource `+0x34`), with the format from
table `0x1434965d0`; the call is context `+0x1c8`. Release follows at
`0x14266e5f5` for actions `0`/`2` through pm `[[R14+0x10]]+0x3b0`. The DSV
resolve is at `0x14266e673` with the DSV block `+0x18` destination and `+0x00`
source, and its release at `0x14266e6a5` when `+0x11` and `+0x29` are `0`/`2`.
The final unbind is `0x14266e6ba`. The `+0x1c8` callee's 4-vs-5-argument
signature is not settled from the image (there is no `d3d11.dll` there); the
D3D11 API's five-argument form is the standard interpretation. RTV entry
`+0x08` and record `+0x30` are not read by `0x0c`/`0x0d`; `+0x30` is read only
by `0x27`/`0x28`.

The pure model is `crates/ow-render::state`: `ViewDesc`, `test_view_alias`,
`SrvCache`/`UavCache`, the six SRV flushes, `unbind_alias`,
`op_0c_alias_unbind` and `apply_render_targets`, with synthetic tests for the
alias predicate, the range bitfield, flush idempotence, the queue, the mask
bit, the DSV-only case and the exact clear/release/bind order. It reproduces
the read per-slot graphics-SRV setter shape and the range-shape CS UAV/SRV
flushes. What remains for the replay is the out-of-band RTV/DSV table (the
record carries pointers), the `0x16..0x1b` cache population and the
queued-SRV binder.

The separate `FUN_142674980` path is a **per-draw state flush**, not the bind.
Opcodes `0x25`/`0x26` call `[cmdlist+0x20 vtable +0x18]` at `0x1426701e9` /
`0x14267027e` before the draw, and `0x27`/`0x28` at `0x1426703bb` /
`0x1426704a4`; the slot is `0x142674980` in the secondary vtable
`0x143497b78` (installed at `[cmdlist+0x20]` by `FUN_142667250` at
`0x142667394`/`0x14266739b`). It unbinds stale UAVs
(`FUN_142679440`), flushes five stage CB/SRV caches (`FUN_14265fc90`), binds
UAV runs (`FUN_142672970`) and flushes five more stages. It never calls
`OMSetRenderTargets` (`+0x108`); `FUN_142672970`/`FUN_142679440` use
`OMSetRenderTargetsAndUnorderedAccessViews` (`+0x110`) with
`NumViews=0xffffffff` and null RTV/DSV, the D3D11 "leave the render targets
unchanged" idiom. The former candidate `0x2d` remains ruled out: at
`0x142670639` it loads the pm object from the replay queue at `+0x38`, shuffles
the 16-byte record payload at `+0x10` into float arguments with a byte
argument, and calls vtable `+0x48` at `0x142670667`; it carries no view or
group state. The engine pass order and the writer of the `0x0c`/`0x0d` records
remain open.

**D4/D5 status.** The header parse, the 64-opcode enum, the full target table,
the missing-opcode path, the executor and the recording sink are pinned by
`cargo test -p ow-render`. The nine end-flush helpers are read but not
executed: the executor defers no bindings, so there is nothing to flush. The
nine read context handlers are tested
argument for argument with synthetic record streams. The D3D11 sink's COM
calls are **not tested**: there is no read record stream and no client
capture. The client's segmented ring (the `+0x00` next pointer and `+0x08`
capacity) is not modelled; the executor takes one contiguous stream and
rejects a record that runs past its end.

**Batch `0x0c..0x1b`: read.** The sixteen state-cache apply/rebuild and bind
handlers were read. `0x0c` and `0x0d` are the render-target bind/unbind pair
(above); the other fourteen remain open because each dereferences an
out-of-band state block or descriptor array. The record layouts are:

| op | payload | state |
|---|---|---|
| `0x0c` | count `+0x18`, RTV array `+0x20` (0x30-byte entries), DSV block `+0x28`, state id `+0x30` | read: the final `OMSetRenderTargets` is at `0x14266e54d`; the arrays are out-of-band; alias-unbind, cache model and `apply_render_targets` are in `ow-render::state`; **implemented in the executor** |
| `0x0d` | none | **implemented**: applies the pending state from `0x0c`, resolves MSAA entries, then unbinds at `0x14266e6ba` |
| `0x0e` | state-block pointer `+0x18` | open: copies 0x58 bytes from `[+0x18]+0x28`; resets five CB counters, six resource counters and the UAV cache via `0x1426790e0` (`+0x110`) |
| `0x0f` | state-block pointer `+0x18` | open: applies the block through `IASetInputLayout` `+0x88`, `IASetPrimitiveTopology` `+0xc0`, `VSSetShader` `+0x58`, `PSSetShader` `+0x48`, `DSSetShader` `+0x200`, `HSSetShader` `+0x1e0`, `GSSetShader` `+0xb8`, `OMSetDepthStencilState` `+0x120`, `OMSetBlendState` `+0x118`, `RSSetState` `+0x158`, pm blend `+0x48` |
| `0x10..0x15` | start slot `+0x1c`, count `+0x20`, descriptor array `+0x28` | open: six constant-buffer caches (VS, HS, DS, GS, PS, CS); `0x142676130` calls `SyncBufferForUsage` per 0x20-byte element and defers the bind to the end-record flush `0x142660620` |
| `0x16..0x1b` | start slot `+0x1c`, count `+0x20`, descriptor array `+0x28` | open: six shader-resource caches (VS, HS, DS, GS, PS, CS); `0x1426781b0` calls `SyncBufferForUsage` per element and the bind is deferred to the per-stage flush (`0x142678bf0`, `0x142678570`, `0x142678710`, `0x142678d90`, `0x142678a50`, `0x1426788b0`) |

The constant-buffer stage order is pinned by the stage byte the replay
initializer `0x142661c40` writes into each cache (`0` VS, `1` HS, `2` DS,
`3` GS, `6` PS, `7` CS) and by the zero-path setter table
`0x143496bf0` (`VSSetConstantBuffers` `+0x38`, `HSSetConstantBuffers` `+0x1f0`,
`DSSetConstantBuffers` `+0x210`, `GSSetConstantBuffers` `+0xb0`,
`PSSetConstantBuffers` `+0x80`, `CSSetConstantBuffers` `+0x238`). The non-zero
bind path uses the pm extension slots `0x3b8..0x3e0` on the object at
replay `+0x10`, not `ID3D11DeviceContext`. The shader-resource flush targets
are `VSSetShaderResources` `+0xc8`, `HSSetShaderResources` `+0x1d8`,
`DSSetShaderResources` `+0x1f8`, `GSSetShaderResources` `+0xf8`,
`PSSetShaderResources` `+0x40` and `CSSetShaderResources` `+0x218`.

The per-draw state-cache apply `FUN_142674980` was also read. It merges the
six SRV caches, calls `FUN_142679440` (UAV bitmask) and `FUN_142672970` (UAV
groups), both of which call `OMSetRenderTargetsAndUnorderedAccessViews`
(`+0x110`), then flushes the five stage caches. It is reached from
`0x25`/`0x26` at `0x1426701e9`/`0x14267027e` before the draw and from
`0x27`/`0x28` at `0x1426703bb`/`0x1426704a4`; it is **not** the render-target
bind and is not reached from `0x00` or `0x0c..0x1b`. The five-stage cache
mapping and the UAV group list are still open.

### D3D11 shader and pipeline binding

| operation | address | evidence |
|---|---|---|
| `pm_dx11::Device::CreateShader` | `0x1426697a0` | log `0x143498000`; descriptor version dword must be `3` (`Invalid Shader Version: %u`); a type byte selects the device call |
| `pm_dx11::Device::CreateRenderPipeline` | `0x142667be0` | log `0x143497e28`; builds the input layout, blend, rasterizer and depth-stencil states |
| `pm_dx11::Device::CreateBuffer` | `0x142665750` | log `0x143497cd0`; builds `D3D11_BUFFER_DESC` and creates each sub-buffer |
| `pm_dx11::CmdQueueImpl::UploadConstants` | `0x1426798c0` | log `0x1434984c0`; dynamic constant-buffer path |
| `pm_dx11::Device::CreateRenderTargetView` | `0x1426687b0` | log `0x143497ed8` |

**Shader objects.** `CreateShader` accepts a descriptor whose version dword is
`3`; the code object's byte length is `+0x30` and its bytecode pointer is
`+0x34`. The type byte at descriptor `+1` maps to the `ID3D11Device` create
calls as follows (the vtable offsets are the Windows API, the type values are
the client's):

| type byte | device call | vtable |
|---|---|---|
| `0` | `CreateVertexShader` | `+0x60` |
| `1` | `CreateHullShader` | `+0x80` |
| `2` | `CreateDomainShader` | `+0x88` |
| `3` | `CreateGeometryShader` | `+0x68` |
| `6` | `CreatePixelShader` | `+0x78` |
| `7` | `CreateComputeShader` | `+0x90` |

Types `4` and `5` go to the invalid-type error. The bytecode is a **DXBC
blob** supplied by the install; there is no compiler and no runtime HLSL path.
Which shader group/code GUID a material picks, and which tier index, is still
**open** ([`shaders.md`](shaders.md)).

**Input layout.** `CreateRenderPipeline` builds a `D3D11_INPUT_ELEMENT_DESC`
array from the pipeline's stream list: each stream has an element count at
`+8` and an element array at `+0x10`; each source element is `0x20` bytes with
a semantic-name pointer at `+0x10` (or the fixed pointer `0x14349310c` when
null), a format byte at `+0x04` and an aligned byte offset at `+0x08`. The
semantic index is read from `+0x00` when the name pointer is null and from
`+0x18` otherwise; the input slot comes from the stream descriptor. The
format byte is resolved through the table `0x1434965d0`. The shader's input
signature comes from the pipeline's shader object at `+0x88` (pointer `+0x28`,
size `+0x30`). The array is passed to `ID3D11Device::CreateInputLayout`
(`vtable+0x58`); failure string `CreateInputLayout Failed`. The element count
is not capped by the read code; what fills the stream list from a model's
semantic bytes is open.

The pipeline also creates `ID3D11BlendState` (`vtable+0xa0`), a rasterizer
state (`vtable+0xb0`) and a depth-stencil state (`vtable+0xa8`) from the
pipeline descriptor. The rasterizer state is where cull mode lives, but no
separate cull, visibility or frustum function is identified. The fixed 64-entry
cap per stream is read; what fills the elements from a model's semantic bytes
is **open** (the model's file-side elements are parsed by `ow-tank`).

**Constant buffers.** `CreateBuffer` aligns a constant buffer's byte size up to
16 (`+0xf & ~0xf`) when the descriptor's `+0x4` flag bit 2 is set; an aligned
size above `0x1000` takes the separate large-buffer path when a device
capability bit is clear. `UploadConstants` finds or creates a
per-buffer dynamic buffer with `D3D11_BUFFER_DESC` usage **2**
(`D3D11_USAGE_DYNAMIC`), CPU access **4** (`D3D11_CPU_ACCESS_WRITE`) and bind
flag **`0x10000`** (`D3D11_BIND_CONSTANT_BUFFER`). The per-draw constant
contents and the update cadence are **open**.

**Draw submission.** The replay's `DrawIndexed` opcode (`0x25`) is the first
draw call proven in the D3D11 path. The `Draw`, `DrawInstanced`,
`DrawIndexedInstanced` and indirect variants are not yet named among the 64
opcodes; the engine's draw-list builder is open. No D3D11
`CreateResources`/indirect-signature path is named by the read strings (the
`DrawIndirect`, `DrawIndexedIndirect` and `DispatchIndirect` signatures are
created by the D3D12 `CreateResources` at `0x142648fb0`); whether the D3D11
backend uses indirect draws at all is **open**.

### D3D11 staged plan

Each stage is one read, one implementation, one test. D1-D4 and the D5
clear/draw/viewport/`0x0c`/`0x0d` handlers are done. The proof is the read
expression plus synthetic tests; a D3D11 capture of the **client** is
**impossible** under the anti-cheat, so no stage is capture-compared.
RenderDoc is used on our own implementation only.

| id | stage | first client function to read | what it produces | proof | state |
|---|---|---|---|---|---|
| **D1** | device + swap chain | `FUN_142666980` `0x142666980`, `0x14267d270`, `pm_dxgi::Instance::InitSwapChain` `0x14262f9d0`, `FUN_141ef6960` | adapter, feature levels, flag expression, `DESC1` width/height/format/count/usage/effect/scaling/alpha/flags, sync interval | value tests pin every field (**passing**); our own D3D11 capture shows the 1280x720 `R8G8B8A8_UNORM` back buffer and a successful `Present` (self-capture, not the client) | **implemented**; no client capture (anti-cheat) |
| **D2** | swap-chain resources | `pm_dx11::Device::CreateSwapChainResources` `0x14266a060` | back-buffer `ID3D11Texture2D`, explicit RTV, optional SRV/UAV, view format from `0x1434965d0`, resize order | value tests pin the table (all 64 entries), the six special cases, the descriptors and the usage gates (**passing**); our own capture shows the back-buffer texture; the client `GetBuffer`/`CreateRenderTargetView`/`CreateShaderResourceView` calls cannot be captured (anti-cheat) | **implemented**; no client capture |
| **D3** | frame begin/end order | `FUN_141ef0600`, `FUN_141ef9c80`, `FUN_141ef0800`, `FUN_141f0caa0` | per-frame step order: begin, wait, prepare/upload, flush + replay, fence wait, per-view submit, end/present | unit test pins the order (**passing**); the runnable logs one line per step; our own capture shows `ClearRenderTargetView` + `Present` as the only calls, and the open steps issue nothing (self-capture, not the client) | **implemented**; only `Present` has an executed body; no client capture |
| D4 | replay dispatch | `FUN_14266c690`, table `0x142671154` | the 64-opcode space, record header `+0x10/+0x14`, missing-opcode path, executor and sink | synthetic record streams (recording sink); no client capture is possible | **implemented**; 11 opcodes executed, 53 open by opcode |
| D5 | clears, render-target bind, viewport | opcodes `0x07`, `0x08`, `0x0c`, `0x0d`, `0x29`, `0x2a` | colour/depth-stencil clear values, RTV/DSV bind and unbind, viewport/scissor rectangle | synthetic record streams for the clear, bind/unbind and viewport/scissor calls | colour/depth clears, viewport/scissor, `0x0c` and `0x0d` **implemented** |
| D6 | draw submission | opcodes `0x25`, `0x26` and the draw-list writer (**open**) | vertex/index counts, start index, base vertex, instance range | synthetic draw-argument tests; the list builder is open | draw opcodes read and synthetically tested; list builder open |
| D7 | input layout | `pm_dx11::Device::CreateRenderPipeline` `0x142667be0` | 64-element cap, per-stream elements, semantic/format/offset/slot, input signature | install parse + synthetic element test | read; material/model join open |
| D8 | shader objects and pipeline state | `pm_dx11::Device::CreateShader` `0x1426697a0`, `CreateRenderPipeline` | DXBC blob selection, type -> stage, blend/rasterizer/depth states | install DXBC + synthetic pipeline test | shader create read; group/tier selection open |
| D9 | constant buffers | `CreateBuffer` `0x142665750`, `UploadConstants` `0x1426798c0` | dynamic CB usage/CPU access/bind flag, 16-byte alignment, contents | synthetic allocation test; contents are open | allocation read; contents open |
| D10 | full-frame image | the whole chain | pixels | **not provable**: the client capture is impossible under the anti-cheat and there is no image oracle | blocked; no pixel stage is claimed |

**Dependency:** D1 -> D2 -> D3 -> D4 -> D5 -> D6/D7/D8/D9 -> D10. D10 is not provable under the anti-cheat blocker.

**What D1 proves.** The creation request is read and pinned: the feature-level
lists, the SDK version, the flag expression, the adapter slot, the default
`DESC1` fields and the sync-interval field chain. The code issues the same
`D3D11CreateDevice` and `CreateSwapChainForHwnd` calls with those values, and
the smoke run created a device and a swap chain on this machine's RTX 4060 at
`D3D_FEATURE_LEVEL_11_1` with the requested descriptor. The creation call is
exercised by the smoke example, **not** by an automated GPU test;
`cargo test -p ow-render` pins the values only. **What it does not prove:** the
device and swap-chain calls cannot be compared against a client capture
(anti-cheat); the three flag inputs and the sync-interval value are runtime
config values whose shipped defaults are not read; the adapter preference sort
is not read; and no pixel is compared. D1 is a pinned creation path, not a
verified renderer.

**What D2 proves.** The back-buffer resource path is read and implemented
operation for operation: `GetBuffer(0)` with the texture-2D IID, the explicit
RTV descriptor (resolved view format, dimension 4, mip slice 0), `GetDesc`, the
SRV descriptor (dimension 4, most-detailed mip 0, one mip level) gated by usage
bit 0, the UAV descriptor (dimension 4, mip slice 0) gated by usage bit 1, and
the release → `ResizeBuffers` → recreate order. The full 64-entry format table
is pinned entry for entry, as are the six explicit special cases; the two
tables are byte-identical and index `0x1f` is 28. The smoke run created the
views on this machine (RTV and SRV present, UAV absent) at both sizes and
recreated them after a resize. **What it does not prove:** the actual
`GetBuffer`/`CreateRenderTargetView`/`CreateShaderResourceView` arguments
cannot be compared against a client capture (anti-cheat); the alternative
engine-format producer `FUN_142636500` is open; a format byte `>= 64` is a read
past the table and is declared open, not mapped.

**What D3 proves.** The per-frame step order is read and pinned: frame begin ->
GPU wait/pacing -> scene prepare -> upload finish (flush + blocking fence wait) ->
command replay (queue vtable `+0x50` -> `pm_dx11` submit -> replay entry) -> fence
wait -> per-view submit -> frame end (composite/blit records, batch records,
per-view submit) -> present. The present path is read exactly:
`FUN_14265e8f0` flushes the queue, then `pm_dxgi::Instance::Present` calls
`IDXGISwapChain::Present(sync interval, flags)` with the four-condition tearing
expression (zero on the default view). The step list drives the
`--renderer d3d11` path and the smoke run for N frames. **What it does not
prove:** the outer caller that orders the three shared frame slots is not read
(steps 1-2 use the vtable order), the D3D11 fence/query wait body is not read,
and no read record stream exists, so only `Present` executes and every other
step is logged as open. No client API call-order trace exists (anti-cheat).

**What D4 proves.** The replay header (`+0x10` opcode, `+0x14` size including
the 0x18-byte header), the full 64-entry target table `0x142671154`, the
64-opcode enum, the missing-opcode path (`0x1434983d0`, continue after the
log) and the record executor are read and pinned. The executor walks a
contiguous stream in the client order, advances by the record size before
dispatch, stops at opcode `0x00` and calls a `Sink` whose methods mirror the
read `ID3D11DeviceContext` calls. **What it does not prove:** the client
segmented ring (`+0x00` next pointer, `+0x08` capacity) is not modelled; a
record that runs past the contiguous buffer is rejected, while the client
reads it; the D3D11 sink's COM calls are not exercised because there is no
read record stream and no client capture.

**What D5 proves.** Nine self-contained handlers are read argument for
argument and implemented: `0x07` `ClearRenderTargetView`, `0x08`
`ClearDepthStencilView`, `0x0c` render-target set, `0x0d` pending resolve,
`0x25` `Draw`/`DrawInstanced`, `0x26`
`DrawIndexed`/`DrawIndexedInstanced`, `0x29` `RSSetViewports`, `0x2a`
`RSSetScissorRects` and `0x30` `Dispatch`. The payload offsets, the client's
Draw-vs-DrawInstanced shortcut (`instance_count == 1 && start_instance == 0`)
and the viewport/scissor field order are pinned by synthetic streams. `0x0c`
carries the resolved RTV/DSV handles to the sink, which performs the per-entry
clear/release, the DSV clear, the alias-unbind of the per-stage SRV/UAV caches
and `OMSetRenderTargets(count, array, dsv)` at `0x14266e54d`. `0x0d` resolves
and releases the saved entries with the read action matrix and unbinds at
`0x14266e6ba`. **What it does not prove:** the per-stage SRV/UAV caches are
modelled but not populated by `0x16..0x1b` in the executor, the queued-SRV
binder is open, and the D3D11 COM calls are not exercised without a read record
stream. The engine pass order that emits the records is open;
`FUN_142674980` is a separate per-draw state flush. The other 53 opcodes are
open by opcode; none is faked.

## The shared engine layer

The frame loop and the command-record substrate are not D3D11-specific. They
apply to both backends.

### The command record stream

The engine does not call the pm device once per draw. It records commands into
a segment ring; the `pm_dx11` replay is a **separate** stream in a different
object, not a translation of this ring.

**The engine ring.** A segment is 0x38 bytes: data `+0x10`, capacity `+0x18`,
completion object `+0x20`, write offset `+0x28`, read offset `+0x2c`, stamp
`+0x30`. The ring is at `cmdlist+0x58`; its current segment index is `+0x258`
and its segment array is `+0x28`. `FUN_141fa7b70` receives the ring itself;
`FUN_141f9fe30` receives a command-list object and reads its ring at `+0x58`.
`FUN_141c232f0` → `FUN_142efe2b0` is the byte writer (a size-dispatched
memcpy). A leaf writes its id and payload, advances `+0x28` under `LOCK XCHG`,
and calls `FUN_141fa9010` to grow/rotate when capacity is short. **Read.**

The four read leaves:

| leaf | id | size | payload |
|---|---|---|---|
| `FUN_141fa7b70` | `0x30` | 4 | none; clears `ring+0x2a8` |
| `FUN_141f9fe30` | `0x1be` | 4 | only when `cmdlist+0x5618 != 0`; clears it and `ring+0x40` |
| `FUN_141f02b10` | `0x387` | 0x28 | `+0x08` base qword, `+0x10` frame-slot qword, `+0x18` zero, `+0x1c` count, `+0x20` zero; `+0x04`/`+0x24` are uninitialised stack bytes |
| `FUN_141f03a80` | `0x60` | 0x18 | `+0x08` `view+0xa0`, `+0x10` 5-bit tag, `+0x11` `0x0d`, `+0x14` `0xffffffff`; `+0x04`/`+0x12` uninitialised |

A per-item leaf in the `0x141f99dd0` region writes a 0x18-byte id `0x352`
record carrying `renderer+0xc1d0` and one item field, then marks the item
`-2` through `0x141f11a30`.

**The two batch buffers.** Both are inline in the renderer object and have
three frame slots:

| | batch A | batch B |
|---|---|---|
| base | `renderer+0x1a0` | `renderer+0xc1d0` |
| frame slots | `+0x1a8 + idx*8` (3) | `+0xc1d8 + idx*8` (3) |
| counts | `+0x1c0 + idx*4` (3) | `+0xc1f0 + idx*4` (3) |
| item arrays | `+0x1d0 + (i + idx*0x800)*8` (3×2048) | `+0xc200 + (i + idx*0x2800)*8` (3×10240) |
| GPU stride | 0x10 | 0x08 |
| direction | CPU→GPU upload (`FUN_141f0edd0`) | GPU→CPU readback (`FUN_141f0e5f0`) |

Batch A copies two qwords per item and sets the item state at `+0x18` to `-3`;
batch B copies one qword per item when its `+0x10` state is `-2`, then sets it
to `-3`. The item bytes are opaque: the read code never interprets the two
qwords or the one qword. `FUN_141efbcb0` is the only consumer of a B item and
returns a visible/culled-style answer `(*item == 0) + 1`; calling B "visibility
or occlusion results" is **inferred**, not read. The writer that stores an item
into `renderer+0x1d0`/`+0xc200` and increments the count was not found; the
likely consumer of the `0x352` records is **open**.

The record layout and both transfers are implemented in
`crates/ow-render::batch` (`Batch387`, `ViewReference60`, `BatchItem352`,
`Batch`, `upload_a`, `readback_b`) and pinned by synthetic tests: the six
`0x387` fields, the two ignored padding dwords, the A/B offsets and capacities,
the 16-byte copy, the 8-byte readback with the `-2 -> -3` state filter, null
items, and over-capacity rejection. The ring consumer and the item meanings are
not implemented because they are not read.

**No id → replay-opcode translation.** The engine ring and the `pm_dx11`
replay stream use different formats and objects: the ring has the id at `+0x00`
with no size field, the replay has the opcode at `+0x10` and the size at
`+0x14`; the ring lives at `cmdlist+0x58`, the replay data at `cmdlist+0x18`
with the offset at `+0x20`. A whole-image scan found **no equality dispatch on
`0x387`** and no table indexed by the record id. The record consumer is
**open**; the ids are not translated into the 64-opcode space.

`FUN_141efa380` (`0x141efa380`, frame-slot table entry `0x1433585e8`, directly
after frame end) advances the frame index and flushes batches A and B; frame
begin also flushes older A slots. The outer order of slots 129–132 is not
proven by a read call site.

### Passes: what exists

**Negative:** no pass-name or render-graph-name string was found. The terms
searched and absent were: `RenderGraph`, `RenderPass`, `BeginFrame`, `EndFrame`,
`BeginPass`, `EndPass`, `RenderTarget`, `SceneColor`, `SceneDepth`,
`ShadowMap`, `PostProcess`, `DepthPrepass`, `BasePass`, `LightingPass`, `TAA`,
`SSAO`, `Bloom`, `Decal`, `Viewmodel`, `Transparent`, `Composite`. The only
`GBuffer.*` strings (`0x143356708`..`0x143356860`) are **DLSS parameters** read
by `FUN_141ee6ea0` (D3D11) and `FUN_141ee73c0` (D3D12), not a pass declaration.
The DLSS parameter path names 16 GBuffer resources; whether the client's own
lighting is deferred is not read.

The stages that are identifiable from structure:

| stage | address | what is read |
|---|---|---|
| scene prepare / buffer update | `FUN_141ef8140` `0x141ef8140` | iterates the renderer's object lists, allocates/updates GPU buffers through the descriptor heap at `renderer+0x48348`, transitions the two batch resources `renderer+0x1a0` and `renderer+0xc1d0` and their three frame slots (`+0x1a8..`, `+0xc1d8..`), and calls `FUN_141ef2260`, `FUN_141f0f230`, `FUN_141f022a0` |
| per-frame upload batch A | `renderer+0x1a0`, frame pointers `+0x1a8`, counts `+0x1c0` | frame end emits one 0x28-byte record id `0x387`; `FUN_141f0edd0` uploads `count * 0x10` bytes from the item pointers |
| per-frame readback batch B | `renderer+0xc1d0`, frame pointers `+0xc1d8`, counts `+0xc1f0` | same record; `FUN_141f0e5f0` reads back `count * 0x08` bytes into pending items |
| final composite / tonemap | `FUN_141df2280` `0x141df2280` | records a full-screen pass and writes a 0x30-byte constant block (`0x141c232f0`); the block's read values include `2.2 / value`, `0.45454544` in the HDR path, 1.0 and 125.0. The field names are open. |
| blit to the back buffer | `FUN_141f97240` `0x141f97240` | chooses a path by target format (`param_2[8] == 0x72` is a special case), emits the record helpers `0x141f93180`/`0x141f938a0` or `0x141f93370`/`0x141f93a90`, then `0x141f96a20` |
| present | `0x142630790` | see above |

**Open:** the world/opaque pass, the GBuffer pass, the lighting pass, the
shadow pass, the transparent pass, the UI pass and the first-person pass are
not identified. The final composite's source texture is a view resource; the
pass that fills it is not read. `0x141df2280` is the only stage whose operation
is clear enough to name, and even there the shader is not read.

### World draw submission (shared negatives)

- **Visibility/culling: open.** No function with a frustum, cull, visibility,
  occlusion or draw-list name was found in the strings. The only `IsProbeCulled`
  (`?IsProbeCulled`, `0x1435c0208`) is Enlighten's light-probe culling, not
  scene visibility. The D3D11 rasterizer state is created but no cull
  policy function is named.
- **Sorting, instancing, per-object constants: open.** No sort-key or
  instance-record function was identified.

## The beta path: D3D12

This section is the previous read, kept because the client can still select
the D3D12 backend, but it is **not** the 1:1 target. The shared `pm_dxgi` swap
chain, frame loop and command-record substrate above apply unchanged.

### D3D12 device creation

| step | function | evidence |
|---|---|---|
| D3D12 instance create | `pm_dx12::Instance::CreateDevice` `0x14267b5b0` | read: logs `pm_dx12::Instance::CreateDevice` (`0x1434989c0`) and `D3D12CreateDevice Failed` |
| D3D12 device create | the d3d12 module object's create entry, called from `0x14267b5b0` | read: `0x14267b5b0` calls the module vtable at slot `+0x78` slot 1, passing the adapter (`param_1+0x38`), flags `0xb000` (or the instance's override at `+0x118`) and the requested parameters |
| device init | `pm_dx12::Device::Init` `0x142654160` | read: logs `pm_dx12::Device::Init` (`0x143492c70`) |
| resources | `pm_dx12::Device::CreateResources` `0x142648fb0` | read: logs `pm_dx12::Device::CreateResources` (`0x143492cd0`) |

The D3D12 instance vtable is `0x1434904a8`: Init slot 1 =
`pm_dx12::Instance::Init` `0x14267b8b0`, slot 3 = `0x142645380`. The D3D12
device `Init` (`0x142654160`) calls `0x14267b5b0`, `0x142648fb0`,
`0x142645b80`, `0x142654a60`, `0x14265b5e0`, plus the lock init `0x142694c30`.
The device-list rebuild is `FUN_142637e20` (log `0x14348f2d0`).

The descriptor heaps are named by the client's own strings → `RTV Heap`
(`0x143492bf0`), `DSV Heap` (`0x143492c00`), `CPU Sampler Heap`
(`0x143492c10`), `CPU General Heap` (`0x143492c28`), `GPU Sampler`/`GPU
General` (`0x143492d08`/`0x143492d28`); the heap path that names them is
`FUN_14263c610` (string xrefs `0x14263c9be`/`0x14263cae7`), reached from the
device init side. `CreateResources` (`0x142648fb0`) creates the command
signatures `DrawIndirect` (`0x143492d58`), `DrawIndexedIndirect`
(`0x143492d90`) and `DispatchIndirect` (`0x143492da8`).

The neutral buffer/RTV/DSV creation is the pm RHI:

- `pm_dx12::Device::CreateBuffer` `0x1426434d0` reads a neutral buffer
  descriptor (`param_3`): byte `+0` is the resource kind, `+4` flags, `+0xc`
  size, `+0x10` usage. It maps the kind through `0x14348f958`, builds a
  `D3D12_RESOURCE_DESC`-style record and calls `CreateCommittedResource`
  (`CreateCommittedResource Failed` is its error string). It is the resource
  allocation layer, not an input-assembler layout.
- `pm_dx12::Device::CreateRenderTargetView` is identified by its log string
  `0x143493118`; its body is in the unanalysed gap between `FUN_142648780` and
  `FUN_142648fb0` (Ghidra has no function entry there; the log xrefs are
  `0x142648c5f`, `0x142648e74`, `0x142648eda`).
- `pm_dx12::Device::CreateSwapChainResources` (log `0x143493340`, log site
  `0x14264b45c`, in the unanalysed gap before `FUN_14264b4e0`) and
  `pm_dx12::Device::CreateSwapChainTexture` (`0x14265c540`, log
  `0x143494260`) are the swap-chain resource side.

### D3D12 pipeline and shader selection

| operation | address | notes |
|---|---|---|
| `pm_dx12::Device::CreateShader` | `0x14264a940` | log `0x143493288`; accepts a shader descriptor whose version dword must be `3`, otherwise logs `Invalid Shader Version: %u` (`0x143493268`); reads the descriptor's code list at `+8` and builds the D3D12 shader object |
| `CreateGraphicsPipelineState` | `0x14265bed0` | log `0x143494160`; called by the device's pipeline create `0x142648700` |
| pipeline library | `0x14265c720` | logs `StoreGraphicsPipelineInLibrary` (`0x143494110`), `CreatePipelineLibrary` (`0x143493fa8`) |
| root signature | log site `0x14264a1e6` | log `pm_dx12::Device::CreateRootSignature` (`0x143493198`); Ghidra has no function entry there |
| sampler | log site `0x14264a8d7` | log `pm_dx12::Device::CreateSampler` (`0x143493248`); Ghidra has no function entry there |

The asset side is read in [`shaders.md`](shaders.md): a material names a group
(`0x085`) and a source (`0x088`), and a group holds N code GUIDs (`0x087`) and N
source GUIDs (`0x040`) in parallel arrays. **Open:** which array index (tier),
which code GUID and which root signature a material picks; the function that
consumes the group is not read. **Negative:** there is no `d3dcompiler` import
or string, and no runtime HLSL compile path. The image carries the string
`Microsoft (R) HLSL Shader Compiler 9.29.952.3111` at `0x143341e08`,
`0x1433420c8` and `0x143355652`; that is an HLSL/DXBC-era marker, but the code
asset's own bytecode header was not read in this pass. Shader bytecode
semantics remain out of scope.

### D3D12 vertex and index layout

- `pm_dx12::Device::CreateBuffer` (`0x1426434d0`) produces a committed
  resource from a neutral descriptor. It does not contain a vertex-element
  list.
- The client's input layout belongs to the pipeline state, and the
  shader-instance asset (`0x086`) is documented as the container that carries
  it
  ([`shaders.md`](shaders.md)). The IA element list is therefore **open**.
- The model's file-side vertex elements are read by `ow-tank`
  (`crates/ow-tank/src/model.rs`): semantic byte, index, format byte, offset,
  stream and stride. That is our parser's reading of the file. The client's
  semantic table is not read; our
  `Vertex` layout (`ow-scene`: position, normal, one UV, four joints, four
  weights) is viewer-only.
- `MRNM` (`MrnmHandler`, `0x141b88f10`) builds the skinning parent/used-bone
  tables, not the IA layout. `MDLC` (`MdlcMaterialRead`, `0x141b88cb0`) is the
  material list. Neither supplies the input elements.

### D3D12 staged plan (beta, not the target)

The previous R0-R11 plan is retained for the beta backend. R0 is shared
substrate; R1-R11 are the D3D12 path and are not the 1:1 target now. The
"capture" and "image" oracles named in the R rows are **unavailable** under
the same anti-cheat blocker as D3D11; they are retained as the beta map, not
as a proof plan. R11 is not provable.

| id | stage | first client function to read | what it produces | proof | oracle |
|---|---|---|---|---|---|
| R0 | command-record append | `FUN_141fa7b70` `0x141fa7b70`; `FUN_141f9fe30` `0x141f9fe30` | record id bytes and cursor advance | synthetic state: one-segment encoder, fixed capacity; bytes and offset compared | **available now**, fixed-capacity domain only; growth/commit edges need an allocator stub |
| R1 | view/swap-chain request defaults | `FUN_141ef6960` `0x141ef6960` | width/height source, format index `0x1f`, buffer count 3, present-object fields, target descriptor | value test against the read constants; descriptor needs a capture | value test now; captured state for the descriptor |
| R2 | swap-chain descriptor | `pm_dxgi::Instance::InitSwapChain` `0x14262f9d0` | `DXGI_SWAP_CHAIN_DESC1` fields, flags `0x40`/`0x800`, swap effect `0`/`4`, buffer count | captured state (hook `CreateSwapChainForHwnd`) | API capture; Unicorn cannot (COM) |
| R3 | device and queues | `pm_dx12::Instance::CreateDevice` `0x14267b5b0`; `pm_dx12::Device::Init` `0x142654160` | adapter, flags `0xb000`, requested parameters, queue/heap counts | captured state (`D3D12CreateDevice` args, heap descs) | API capture |
| R4 | frame loop order | `FUN_141ef0600` `0x141ef0600`; `FUN_141ef0800` `0x141ef0800` | fence wait, update, submit, present order; marker ids 1..8 | trace comparison (API call order) | API capture |
| R5 | command-list submission | `FUN_141f0caa0` `0x141f0caa0` | per-view list array, resource transitions, submit/wait | captured state (`ExecuteCommandLists` order, fences) | API capture |
| R6 | pass list and render targets | `FUN_141ef8140` `0x141ef8140` plus the per-pass recorder (**open**) | pass order, RT sets, clears, viewport | captured RT-bind trace; image | API capture + image |
| R7 | world draw list | writer of `renderer+0x1a0`/`+0xc1d0` and the engine-ring consumer (**open**); `0x387` is read as batch A/B, not a draw list | visibility, batches, sort keys, draw args, instancing | synthetic if the list is pure; captured argument buffers | API capture |
| R8 | per-object constants | the constant writer inside prepare/the pass recorder (**open**) | constant-buffer layout and values | captured CB contents compared bit-exact | API capture |
| R9 | pipeline and shader tier | `pm_dx12::Device::CreateGraphicsPipelineState` `0x14265bed0` plus the engine cache caller (**open**) | code GUID, tier index, root signature, PSO key | asset join (install) + captured PSO descriptor | install + API capture |
| R10 | vertex/index layout | the model render-mesh loader (**open**; `MRNM` `0x141b88f10` is skinning, not IA) | IA elements, strides, index format, submeshes | install parse + captured IA state | install + API capture |
| R11 | full-frame image | the whole chain | pixels | **not provable** (anti-cheat) | no image oracle |

**Dependency:** R0 -> R1/R2 -> R3 -> R4/R5 -> R6 -> R7/R8 -> R9/R10 -> R11 (D3D12
beta only). The capture/image oracles in the R rows are unavailable (anti-cheat).

## Proof: what the current harness can and cannot do

The differential harness (`dev/verify_harness`) runs a client function under
Unicorn with **plain memory buffers and at most four register arguments**
(`emu_oracle.py`, `RCX/RDX/R8/R9`). It has no COM, no D3D11 device, no DXGI
swap chain, no GPU, no allocator vtable and no TLS beyond what the fixture
sets. A function that enters `pm_dx11`, `pm_dx12`, `pm_dxgi` or a driver call
cannot be driven at all; a function that grows a segment through
`FUN_141fa9010` cannot either.

This gives two proof classes for the stages above:

- **read** - the client's own expression, address and conclusion, recorded in
  this document and the relevant `docs/formats/<system>.md`;
- **synthetic** - a pure unit test that drives the read expression with
  hand-built state. For D4/D5 this is a synthetic record stream into the
  executor with a recording sink as the oracle. No GPU, no COM and no client
  capture is involved.

There is **no captured-state class and no image class**. A capture of the
client is impossible: the anti-cheat makes attaching RenderDoc/PIX to the
client, or launching it under a capture tool, an account-ban risk. Running or
hooking the game to obtain a call trace is therefore not a permitted route,
and this project does not plan around it. The image oracle is **unavailable**
for the same reason. D10 and every pixel stage are **not provable** under this
blocker; no screenshot, cross-GPU or otherwise, is a substitute.

### RenderDoc on our own implementation (not a client comparison)

RenderDoc v1.46 is installed on this machine (`C:\Program Files\RenderDoc`).
A D3D11 capture of **our** `d3d11_smoke` example exists and is described
below. It proves only that our own code issues the pinned calls; it is **never**
compared against the client and cannot close a client-behaviour claim.

`renderdoccmd capture` of `ow-render`'s `d3d11_smoke` example, with the in-app
trigger (`OW_RENDERDOC_CAPTURE=<template>`), produced a D3D11 capture. Compared
through the RenderDoc MCP:

- API **D3D11**; one texture, the back buffer, **1280x720 `R8G8B8A8_UNORM`**;
- actions: `ID3D11DeviceContext::ClearRenderTargetView()` then `Present`;
- no draws, which is correct for D1-D3: the step list executes only the
  placeholder clear and `Present`, and every other step is logged as open.

Two capture-tool facts, recorded so the next run does not rediscover them:

- RenderDoc v1.46 returns a **null immediate context** from `D3D11CreateDevice`
  when the `PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY` flag (`0x80`) is set.
  Capture runs therefore set `OW_D3D11_ALLOW_LAYER=1`; that input's shipped
  value is unread, so no read value is changed by the capture.
- `renderdoccmd capture` does not capture a frame by itself. The smoke example
  loads `renderdoc.dll` and calls `RENDERDOC_TriggerCapture` (API 1.6.0, slot
  15) when `OW_RENDERDOC_CAPTURE` names the output template.
## Open

- The per-frame pass sequence: world/opaque, GBuffer, lighting, shadow,
  transparent, UI, first-person. No pass names exist. D4/D5 name the colour
  clear (`0x07`), depth-stencil clear (`0x08`), render-target bind (`0x0c`),
  MSAA resolve + unbind (`0x0d`), viewport (`0x29`), scissor (`0x2a`), draw
  (`0x25`/`0x26`) and compute dispatch (`0x30`) opcodes. The remaining next
  anchors are the engine-ring consumer and the pass writer that emits
  `0x0c`/`0x0d`; there is **no** record-id -> replay-opcode translation to
  find. The per-draw `FUN_142674980` state flush is a separate, still-open
  path.
- The **outer caller of the frame slots**: who invokes slots 129 (begin), 130
  (wait), 131 (end) and 132 (`FUN_141efa380`, advance/flush), and in what
  order. `Step::ORDER` uses the vtable order (129 -> 130) and the read call
  chains inside each function; the begin/wait adjacency is not proven by a
  read call site.
- The **D3D11 fence/query wait** body: `pm_dx11::CmdQueueImpl::Init` creates a
  query (type 0) and `Begin`s it, but the wait/`GetData` side of the pm fence
  object is not read.
- The **per-draw state flush** `FUN_142674980` (secondary vtable
  `0x143497b78` slot `+0x18`): the UAV group list/bitmask, the five-stage
  CB/SRV cache mapping and the `0x27`/`0x28` state-id use are open. It is
  called from `0x25`/`0x26`/`0x27`/`0x28`, not from `0x0c`/`0x0d`.
- The **`0x0c` alias-unbind sub-path** is read and modelled: it unbinds SRVs
  that alias the current RTV/DSV across the five stages, CS SRV and CS UAV.
  Its inputs are the replay's per-stage caches (`0x29a0`, `0x49d0`, `0x6a00`,
  `0x8a30`, `0xaa60`, CS SRV `0xca90`, CS UAV `0xeef0`), which opcodes
  `0x16..0x1b` and the UAV path populate. The cache model is in
  `ow-render::state`; what remains is the out-of-band RTV/DSV table, the
  executor cache population and the queued-SRV binder.
- The **`0x16..0x1b` cache population** is not implemented. `FUN_1426781b0`
  creates an `ID3D11Buffer`; the handler compares the cache slot at `+0x29a0`
  with the element's first qword and, when different, enqueues the 16-byte
  descriptor. It does **not** write the slot. The binder that writes the slot
  and issues the non-null `*SetShaderResources` call is not read, so do not
  populate the caches from the record until it is found.
- A swap-chain RTV bind call site at frame start is also not read.
- The **engine-ring consumer**: the function that walks the `0x352`/`0x387`
  records and turns them into GPU work, and the writer that stores an item
  into `renderer+0x1d0`/`+0xc200` and increments `+0x1c0`/`+0xc1f0`. No
  equality dispatch on the record id exists; the ids are not translated into
  the 64-opcode space.
- The **53 open replay opcodes**: each is named with its read gap in
  `crates/ow-render::replay` and the D4/D5 table above. The largest groups are
  the deferred constant-buffer/shader-resource caches (`0x10..0x1b`,
  `0x1e..0x24`), the pm state-cache calls (`0x0e`, `0x0f`, `0x2b..0x2f`), the
  resource copies (`0x01..0x0b`) and the query/annotation path (`0x31..0x3f`).
  `0x0c`/`0x0d` are implemented. The caches are modelled but not populated, and
  the queued-SRV binder is open. No read record stream exists in the crate yet,
  so the D3 `CommandReplay` step remains open.
- The `0x387` record is read as a batch submission (`base`, frame slot,
  count), not a draw, descriptor update or constant upload. The batch A item
  is 16 opaque bytes; the batch B item is one opaque 8-byte value with a
  readback state machine. Their world/UI purpose and item meaning are open.
- Visibility, sorting, instancing and per-object constants; the rasterizer
  state's cull mode field mapping.
- The material → shader-group → code GUID/tier selection and the root
  signature/PSO key (both backends).
- The D3D11 input-layout element producer: the pipeline record is read, the
  model/material join that fills it is not.
- The D3D11 adapter preference sort (`FUN_14262ba20`) and the engine backend
  kind byte's producer.
- The two D3D11 device flags whose inputs are runtime config: debug and
  allow-layer-altering; the single-thread flag's producer; the
  `SetMaximumFrameLatency` value.
- The sync-interval boolean's producer (the config global at the obfuscated
  address) and its shipped default.
- The swap chain's second format path `FUN_142636500` (HDR?) and the exact
  swap-effect/fullscreen flag conditions. The view-format byte is indexed into
  the 64-dword table `0x14348bf10` and the D3D11 view table `0x1434965d0`
  (byte-identical in this image) with no range check; a byte `>= 64` reads past
  the table. No shipped path that produces one was seen, so `ow-render`'s
  `view_format` returns `None` there instead of mapping adjacent memory.
- The `UpdateSwapChain` fullscreen/target-mode branch (`FindClosestMatchingMode`,
  `SetFullscreenState`, `ResizeTarget`): the windowed release → `ResizeBuffers`
  → recreate path is implemented; the fullscreen path is not.
- The GPU marker phase names (ids 1..8 are read; names are absent).
- The renderer's own shaders: bytecode is out of scope; the code asset's
  DXBC/DXIL header was not read.

## Verification

```text
python dev/verify_harness/cli.py list
python dev/verify_harness/cli.py fn/RenderCommandAppend    # when R0 exists
cargo test -p ow-render
cargo run -p ow-render --example d3d11_smoke
cargo xtask verify --fast
```

The D1-D3 smoke path prints the created adapter, feature level, flag word and
the swap-chain descriptor, the D2 view descriptors (RTV/SRV/UAV), and one
`renderer: D3:` line per frame step; it presents two frames, resizes, prints
the recreated D2 views and presents once more. It does not use RenderDoc or
PIX and does not compare pixels. `cargo test -p ow-render` also runs the
D4/D5 synthetic record-stream tests: the full 64-target table, the header
parse, every implemented handler's exact arguments, the missing-opcode path,
the truncated-record errors, the open-opcode assertion and a mixed-stream call
order. The engine batch model adds tests for the `0x387`/`0x60`/`0x352`
layouts, the A/B offsets and capacities, the 16-byte upload, the 8-byte
readback and the over-capacity rejection. The state model adds tests for the
alias predicate, the range bitfield, flush idempotence and queue preservation,
the `0x0c` order, the mask bit, the DSV-only case and
`apply_render_targets` clear/release/bind order. The D3D11 sink's COM calls are
not exercised: there is no read record stream and no client capture. The runnable client path prints the same D2
line and the same D3 step log for `--d3d11-frames N`. The viewer's current
behaviour is checked by the existing scene tests, not by this document:

```text
cargo test --release -p ow-scene --test practice_launch -- --nocapture
cargo run --release -p ow-client
cargo run --release -p ow-client -- --renderer d3d11 --no-map --d3d11-frames 2
```

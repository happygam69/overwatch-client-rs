# The renderer: the `pm` RHI, its backends, and what the capture does not hold

## Current state

- **Data surface: read** - the in-house `pm` RHI, D3D11/D3D12 backends,
  DLSS/XeSS/FSR/AGS integrations, the 16-attrib G-buffer, the PSO library
  and DXR hooks.
- **Code surface: not readable from the standing image** - the renderer
  code is control-flow flattened in the captures; the first-person pass code
  was not recovered.
- **Viewer:** textured, skinned draws are real (skeleton Lookup, materials
  where the keyring allows); the shading model is a stand-in.

**Verify:** `cargo run --release -p ow-view -- --play --hero 3` (eye-check);
`cargo test --release -p ow-tank --test bc6h`.


The rendering pipeline was attacked through its string anchors. The **data
surface** is fully readable and identifies the architecture; the **code
surface is absent from the standing capture**, which is itself the finding.
Addresses are v2b session coordinates; range = v2b − `0x17AEC0000`, exec = v2b
− `0x71820000`.

## What the renderer is

An in-house render hardware interface code-named **`pm`**, with two backends
over a DXGI layer. Its own debug strings enumerate the API surface:

* **`pm_dxgi::Instance`** — `Init`, `InitGpus`, `QueryGpu`, `InitSwapChain`,
  `UpdateSwapChain`, `Present`, `SetVideoMemoryReservation`, `GetStatus`, and
  **`DeferredStreamlineSetup`** (the NVIDIA Streamline host that carries DLSS
  integration).
* **`pm_dx12::Device`** — the resource surface: `CreateBuffer`,
  `CreateBufferView`, `CreateConstantBufferView`, `CreateTexture`,
  `CreateTextureView`, `CreateTextureFromResource`, `CreateRenderTargetView`,
  `CreateRWBufferView`, `CreateRWTextureView`, `CreateSampler`,
  `CreateHeap`, `CreateDescriptorTable`, `CreateQueryPool`, `CreateGpuFence`,
  `CreateCmdPool`, `CreateShader`, `CreateRootSignature`,
  `CreateGraphicsPipelineState`, `CreateComputePipeline`,
  **`CreateRaytracingPipeline`**, **`Dx12CreatePipelineLibrary`**,
  `RegisterMemoryBudgetChangeNotification`, `Dx12RegisterMessageCallback`.
* **Command submission by trait-specialised templates**:
  `CmdListImpl<...Traits>` / `CopyCmdListImpl<...>` for `CopyCmdListTraits`,
  `ComputeCmdListTraits`, `BundleCmdListTraits`, and
  `CmdQueueImpl<...Traits>::Init/FlushInternal/FinishInternal` — i.e. one
  implementation per command-list kind, three queues (copy/compute/graphics).
* The debug strings also cover the D3D12 entry points the module resolves
  dynamically (`D3D12CreateDevice`, `D3D12GetDebugInterface`,
  `D3D12SerializeVersionedRootSignature`), DXGI factory creation
  (`CreateDXGIFactory1/2`, `DXGIGetDebugInterface1`), and the DXGI live-object
  checks and `CreateSwapChainForHwnd` failure paths.

99 `pm_dx*` strings in total; D3D12 is the primary backend, D3D11 the
compatibility one.

## Vendor integrations that are in the binary

* **DLSS** — Streamline host (`DeferredStreamlineSetup`) plus the `nvngx.dll`
  fallback ("failed to load %ls - will now try nvngx.dll") and the DLSS
  dynamic-resolution knobs (`DLSS.Get.Dynamic.Max/Min.Render.Width/Height`).
* **XeSS** — `xessD3D11CreateContext/Init/Execute`, `xessD3D12*`.
* **FSR / AMD FFX** — the host API is present by name:
  `ffxCreateContext`, `ffxDestroyContext`, `ffxConfigure`, `ffxQuery`,
  `ffxDispatch`.
* **AMD AGS** and **Intel INTC** driver extensions
  (`agsDriverExtensionsDX11_SetDiskShaderCacheEnabled`,
  `_INTC_D3D12_CreateComputePipelineState`, `..._AddShaderCachePath`).

So the shipped renderer is a D3D12-first deferred pipeline with all three
vendor upscalers wired in as optional hosts.

## The rendering model

The G-buffer is named in the debug layer, which is why its layout survives in
`.rdata`:

```text
GBuffer.Albedo  GBuffer.Roughness  GBuffer.Metallic  GBuffer.Specular
GBuffer.Subsurface  GBuffer.Normals  GBuffer.ShadingModelId  GBuffer.MaterialId
GBuffer.Attrib.8 … GBuffer.Attrib.15           (16 attributes total)
TonemapperType
```

Deferred shading, 16 G-buffer attributes, a material/shading-model id pair, and
a selectable tonemapper. The game-side renderer knobs found: config vars
`render.uiLuminanceScale` (v2b `0x7ff7c887f104`), `render.OcclusionQueryMinDist`
(v2b `0x7ff7c88bb82c`), and debug panels `PSOStatusOverlay`,
`ShaderRootSignatureResources` (v2b `0x7ff7c88bd234`, `0x7ff7c88b6774`).

## The one renderer function that is reachable

The PSO debug window ("PSO GROUP COLLECTION", v2b `0x7ff7c886b964`) is
registered by a one-shot stub whose body was read by disassembly
(v2b `0x7ff7c6a9c700..0x7ff7c6a9c7ce`; range `0x7ff64bbdc700..`, region
`7ff64bbd7000_00024000_0x00040000`, exec `0x7ff75527c700..`). It decodes a
name record in place (`xor byte [rcx+0xc], al` against the key at
`0x7ff7c886b958`), then builds a UI panel object at `RDI`: title
`"PSO GROUP COLLECTION"` at `+0x20`, callbacks at `+0x50`/`+0x60`/`+0x68`
(`0x7ff7c6a9d590` is the draw callback) and `+0x98`, the shared no-op at
`+0x70` (`0x7ff7c525a030`), counts `+0x28 = 0x27`, flags at `+0x38`/`+0x3c`,
and a call to the panel registration at `0x7ff7c6d55610`. That is the PSO
debug overlay, not the render path.

## What the capture does not hold (the finding)

**No call site references any renderer string.** For each of five sampled
strings (`pm_dx12::Device::CreateGraphicsPipelineState`,
`pm_dxgi::Instance::InitSwapChain`, `pm_dxgi::Instance::UpdateSwapChain`,
`GBuffer.Albedo`, `D3D12CreateDevice`) there are **zero** RIP-relative
references in the module image and **zero** 64-bit or 32-bit pointers in any
captured data. The debug strings themselves are present, but nothing in the
standing dump uses them:

* the message strings are not referenced through a pointer table (the message
  catalog theory is refuted by the data scan), so
* the code that emits them is simply **not in the captured pages** — the
  renderer's executable pages were not resident/committed in the captured
  session's address-space snapshot, the same lazy-mapping behaviour the
  project already records for cold code.

Consequence: the renderer's *behaviour* (deferred pass structure, PSO cache
policy, upscaler configuration, swapchain policy) cannot be read from the
standing captures, no matter how many strings are mined. It needs a capture
taken with the renderer's code resident — the same D3D device creation and
first frames the game runs at startup — and then the `pm_dx12::Device` /
`pm_dxgi::Instance` functions above are the entries to read, by name, from the
strings' neighbours.

**Update (2026-10-19):** the warm captures *do* contain the RHI. The merged
warm image has **43 RIP-relative references** to the renderer strings at v2b
`0x7ff7c73b7xxx…0x7ff7c73bbxxx` (for example `pm_dxgi` at
`0x7ff7c82ca310`), and the D3D12/DXGI functions around `0x7ff7c73b7e30` are
readable. The "not in the captured pages" paragraph above describes the old
standing dump only.

## Method

String sweep (`search_strings`) over the module image for the renderer's
vocabulary; RIP-relative scan and data-pointer scan for five anchors to
establish referentiality; Ghidra window disassembly for the PSO registration
stub; REA over a minimal PE of the region (`dev/mkpe.py`, sha256
`25383094f7a8…`) to attempt a second read — REA's analysis of the region
created functions around it but not that stub, so the disassembly window is the
read. The G-buffer and API-surface lists are quoted from the captured data,
not inferred.

## The texture formats the install ships, by bytes per pixel (2026-10-15)

`ow-tank::texture` decodes BC1/BC2/BC3/BC4/BC5/RGBA8. What that leaves is not
"~10% BC7": it is most of the install. A **spread** sample of 900 textures
(`ow-tank-tex --unknown 900`; every 228th of the 205,512 texture assets)
against a spread census of 800 (`--census 800`) gives:

| stored byte | DXGI id | textures in 900 | bytes per pixel | identification |
|---|---|---|---|---|
| 100 | 99 | 189 | 1.00 | **BC7** sRGB (`BC7_UNORM_SRGB`) |
| 10 | 10 | 177 | **8.00** | **`R16G16B16A16_FLOAT`** |
| 96 | 95 | 70 | 1.00 | **`BC6H_UF16`** (HDR) |
| 51 | 50 | 49 | **2.00** | **`R16_FLOAT`** |
| 31, 35 | 30, 34 | 2 | 4.00 | `R8G8B8A8_UINT` / `R16G16_FLOAT` |
| 73-84 | 72-83 | 319 | BC | BC1/BC4/BC5 — decoded today |
| 29, 30 | 28, 29 | 5 | 4.00 | `R8G8B8A8_UNORM/SRGB` — decoded today |
| — | — | ~1% | — | content key absent |

The bytes-per-pixel column is what identifies the format: a texture's level-0
byte count over its pixel count. Only single-mip textures give a clean reading
(a full mip chain totals 4/3 of level 0, and an asset with a separate payload
falls back to the asset length), which is why the table quotes the `256×1 …
256×10` ramp-shaped textures for the 8 B/px row: those are 8.00 exactly.

So the four formats worth writing, in order of how much they unlock:

1. **BC7** (21%) — also exactly what both of hero index 3's textures are, so it
   is the decoder that puts a first real texture on a model.
2. **`R16G16B16A16_FLOAT`** (20%) — uncompressed, trivial to decode, and the
   shapes it appears in (256×1/3/5/10) look like the ramp/LUT strips.
3. **`BC6H_UF16`** (8%) — HDR; needed for skyboxes, not for a hero. **Done** —
   see § "BC6H is decoded, and checked against `bcdec`".
4. **`R16_FLOAT`** (5%) — single channel, likely height/displacement.

The `+1` offset from DXGI below id 26 is confirmed again here: stored 10 reads
as 8 B/px = DXGI 10, not DXGI 9 (`TYPELESS`, which would be a different
interpretation of the same size).

## BC7 is decoded, and checked against a second decoder (2026-10-15)

`ow-tank::texture` now decodes BC7 (all eight modes, the 64 2-subset and 64
3-subset partition patterns, the anchor tables and the interpolation weights),
`R16G16B16A16_FLOAT`, `R16_FLOAT` and `R16_UNORM`. A spread census of 600
texture assets now reads **541 (90%)**, up from ~40%; what is left is **BC6H**
(51, 8.5%) and 8 assets whose content keys are absent.

The tables are data, not derivable code: they are the BC7 specification's, taken
from the public-domain `bc7decomp` reference. **A transcription of tables cannot
be verified by reading it**, so the decoder is checked against an *independent*
implementation over real assets:

* `ow-tank-tex --dump FILE` writes the *exact* level-0 block run a decode used
  next to its own RGBA output, and `dev/tex_oracle.py` feeds those same bytes to
  `texture2ddecoder` (a separate C port of the same specification, installed
  outside the tree) and compares pixel for pixel.
* **39 of 39 spread dumps are identical** across BC1, BC4, BC5 and BC7 — the one
  documented difference is BC1's 3-colour mode, where the oracle paints index 3
  opaque black and the D3D specification (and Microsoft's own `DecodeBC1`)
  specify transparent black, so the project keeps transparent.
* Both of hero 3's own textures are **pixel-identical** to the oracle over
  65,536 and 262,144 pixels.

Two things the check turned up, both of which had been silently wrong:

1. **565 expansion must be bit replication** (`(r5 << 3) | (r5 >> 2)`), not a
   scale by 255/31. Rounding reads one level high on many values — red 58 where
   replication gives 57 — and every one of those pixels disagreed with the
   oracle.
2. **A single-channel format keeps its channel position.** BC4 decodes into
   red with green and blue zero (what the hardware hands a shader), not into
   three equal channels; a viewer that wants a greyscale *picture* promotes it
   at the point of display.

The float formats cannot be checked this way (the oracle has no such modes), so
they are only checked for shape and by looking at the images they produce — said
plainly because a "verified" claim here would be false.

**What the check does not cover:** BC6H — which is implemented now (§ "BC6H is
decoded, and checked against `bcdec`") but which this oracle cannot arbitrate,
because `texture2ddecoder` returns 8-bit clamped BGRA for it; and any block
whose mode the oracle happens not to hit. `--dump-spread` samples across the
whole type, which is what makes the coverage claim meaningful.

## What is not textured, and why (2026-10-15)

Three separate things are worth keeping apart:

* **Map props** have no readable material: type `0x008` is 16 of 2000 readable
  on this install (a keyring limit, `materials.md`). They draw with the 1×1
  white texture and the viewers say so.
* **A hero's *body*** is in the same state, which is *new* this round: the model
  component carries a look (field `0x9297ADB4`, type `0x01A`), and of that
  look's 8 materials **5 are not readable on this install** — including the one
  behind the body's main 19,596-triangle submesh — and **one more names no
  colour map**. A first version of this note said "3 of 9 submeshes textured";
  that was measured further and is **too generous**: those materials' texture
  lists hold *data* maps, and their mean colours say so —

  | material texture | what it is |
  |---|---|
  | `0x0C000000000027ED`, 128×128 BC5 | mean RGB (126, 126, 0): a two-channel normal/mask map |
  | `0x0C0000000000039C`, 256×256 BC7 | mean RGB (0, 0, 0): colour channels empty, an alpha/mask map |
  | `0x0C00000000005449`, 128×128 BC1 | mean RGB (35, 34, 35): dark grey, not an albedo |

  Two more of the surfaces resolve to a texture that is **a flat fill** — a
  128×128 BC1 at mean RGB (35, 34, 35) with **sd 0.0**, and a 256×256 BC7 at
  (0, 0, 0) with sd 0.0 — which is what painted a *black patch* on each forearm
  plate (16 triangles each) until the rule below caught it.

  So the rule the harness follows is "base colour is the material's first
  **sRGB** entry; a material without one paints nothing, and a texture with no
  structure (per-channel standard deviation < 2) paints nothing either" —
  because a normal map or a flat fill rendered as albedo *looks* like a texture
  while being wrong, and black patch reads as "the game authored this black"
  rather than "the colour is not readable here". Consequence on this install:
  the hero draws **entirely on the placeholder** — `0 of 9 submeshes` — and the
  harness prints the per-submesh accounting plus every texture's mean and
  standard deviation behind that. `--uv-check` swaps the placeholder for a UV
  checker so an untextured surface cannot be mistaken for a textured one.
* **The two textures the hero asset names directly are face/portrait *UI*
  assets**, not body textures. Painting a whole model with them was this round's
  visible mistake: it reads as "Tracer-ish" and is wrong. The hero's *named*
  textures are decoded and correct; they are simply not what a body samples.

Consequence for the plan: a *fully* textured hero needs a build (or keyring)
whose `0x008` keys are present — the code path is in place and would light up
unchanged.

## The exact texture gap, measured (2026-10-15, re-measured 2026-10-17)

`ow-tank-tex --census 600` (a spread sample across the whole type) breaks the
failures down completely. **After BC6H landed:**

| outcome | count | share | why |
|---|---|---|---|
| decoded | **592** | **98.7%** | BC1/2/3/4/5, BC6H, BC7, R8G8B8A8, R16G16B16A16_FLOAT, R16_UNORM |
| content key absent | 8 | 1.3% | not installed on this machine (the content split `materials.md` documents) |

Before BC6H the same census read 541 (90.2%) decoded with **51 (8.5%) BC6H
unreadable**; those 51 now decode, so the only thing still missing is ~1% of
files that are simply not present locally. Nothing else fails; there is no third
class. (*install*, measured.)

## BC6H is decoded, and checked against `bcdec` (2026-10-17, *published-check* + *install*)

BC6H is the HDR member of the family and was the last format on this install we
could not read. It is now decoded in full: all fourteen modes, the 2-or-5-bit
mode field, the 32 two-subset partition shapes with their fix-up pixels, delta
("transformed") endpoints, both unquantisation fix-ups, and half-float output.

**The tables are data, not derivable code**, so they are re-derived rather than
trusted. `dev/bc6h_tables.py` reads three published sources live — Khronos
`EXT_texture_compression_bptc` (revision 2), iOrange's `bcdec.h`, and
Microsoft's "BC6H Format" page — and prints what each says:

* `Table.MF`: all fourteen rows match ours (mode number, transformed flag,
  partition-bit count, endpoint precision, the three delta widths).
* `Table.F`: all fourteen modes' *exact field order* matches ours, field by
  field, including the three fields stored most-significant-bit first.
* `Table.P2` / `Table.A2`: Khronos's first 32 partition rows and their fix-up
  pixels agree row for row with `bcdec`'s, and that first half is exactly what
  BC6H uses — which is why the decoder shares BC7's tables instead of keeping a
  second copy. The in-file test pins our shared rows against that listing.
* Microsoft's page confirms the per-mode bit counts (75/72 endpoint bits +
  5 partition + 46 index bits for the two-region modes; 60 + 63 for the
  one-region ones) and the reserved-mode list (19, 23, 27, 31).
* Script exit code 0 = every source agrees with every other, so this is a
  *published-check*, not a reading of our own table back to itself.

One field was worth naming because a third-party source prints it differently:
**mode 9 (stored 26) stores its ninth field as `b3[5]` — blue's sixth delta
bit.** `Table.MF`'s `{5, 5, 6}` row settles it on its own (blue carries the
six-bit delta), and Khronos revision 2's `Table.F` prints `b3[5]` too.

**The arithmetic is checked against an independent decoder, in half floats.**
`ow-tank-tex --bc6h-selftest` writes, for every one of the fourteen modes, 64
deterministic pseudo-random blocks and our own decode of them;
`dev/bc6h_oracle.py` hands the same bytes to `bcdec` (iOrange's reference
implementation, which follows the specification's steps directly) and compares
**16-bit half floats, bit for bit**:

| sweep | blocks | half-float channels | result |
|---|---|---|---|
| synthetic, all 14 modes, `UF16` | 896 | 57,344 | **0 differ** — every mode identical to `bcdec` |
| synthetic, all 14 modes, `SF16` | 896 | 57,344 | **0 differ** — every mode identical to `bcdec` |
| real install assets, `UF16` (`--dump16-spread 300`) | 21 assets, 36,240 blocks | 2,319,360 | **0 differ** |

Real encoder output matters as much as synthetic input: encoder output reaches
combinations a random payload rarely does, and both are checked. **All fourteen
modes are exercised by real assets**, not only by the synthetic sweep (blocks
per mode over the 21 assets):

| stored mode | blocks | share | | stored mode | blocks | share |
|---|---|---|---|---|---|---|
| 11 (spec 13) | 13,871 | 38.3% | | 1 (spec 2) | 136 | 0.4% |
| 0 (spec 1) | 5,256 | 14.5% | | 30 (spec 10) | 2 | 0.01% |
| 14 (spec 6) | 3,615 | 10.0% | | 15 (spec 14) | 20 | 0.06% |
| 7 (spec 12) | 3,610 | 10.0% | | | | |
| 2 (spec 3) | 2,616 | 7.2% | | | | |
| 6 (spec 4) | 2,213 | 6.1% | | | | |
| 10 (spec 5) | 1,968 | 5.4% | | | | |
| 18 (spec 7) | 1,323 | 3.7% | | | | |
| 3 (spec 11) | 654 | 1.8% | | | | |
| 26 (spec 9) | 528 | 1.5% | | | | |
| 22 (spec 8) | 428 | 1.2% | | | | |

No block in any of the 36,240 carries a reserved mode number.

### The stored byte and the signed flag: resolved, and by measurement

DXGI 94 is `BC6H_TYPELESS`, 95 is `BC6H_UF16` (unsigned endpoints) and 96 is
`BC6H_SF16` (signed). The client stores its format byte one higher from 26 up,
so a **stored 96 is `BC6H_UF16`**, a stored 97 would be `SF16`, and a stored 95
is the typeless id (decoded as unsigned, recorded as a choice). Getting this
wrong silently mis-decodes every HDR value, so it was not taken on trust:

* **What the install carries** (*install*): the 600-asset census has 51 BC6H
  assets and **every one stores byte 96**; none stores 97. So under the −1 rule
  they are all `UF16`. (The census also confirms the rule itself, which was
  established for BC7 and for the uncompressed families.)
* **What the pixels say** (*install*, measured): the alternative reading — that
  the stored 96 means DXGI 96 and the −1 offset does not apply to BC6H, making
  these `SF16` — is refuted by decoding both ways and looking at the numbers.
  Over the 21 assets (2,319,360 channels):

  | | as `UF16` | as `SF16` |
  |---|---|---|
  | negative channels | **0** | 25,360 (up to 49.4% of one mode's channels) |
  | largest value in the image | 1.00 (12 of the 21 never exceed it) … 3.19 | 65,504 (the largest finite half) |
  | mean value | 0.26 … 0.68 | 7 … 6,260 |
  | mean difference between neighbouring pixels, per mode | 0.0002 … 0.078 | 12 … 1,568 |

  Read as unsigned, each mode's pixels form a smooth image whose scale is a
  skybox or a light probe. Read as signed, the same bytes are noise three to
  five orders of magnitude rougher, with values at the top of the half range.
  The unsigned reading is the one that decodes these files.

A second observation falls out of the mode histogram and is worth recording
because it contradicts a common reading of the format: **the install's encoder
uses all fourteen modes for unsigned data**, including the four modes (stored
3, 7, 11, 15) that Microsoft's reference encoder reserves for the signed
variant — 50.1% of the blocks here are from that set (18,155 of 36,240). So the
mode number does *not* imply the variant on this install; the DXGI id does, and
every one of these files decodes correctly as `UF16`.

### What `texture2ddecoder` can and cannot say about BC6H

`dev/tex_oracle.py` (the 8-bit oracle the BC7 work uses) **refuses to judge
BC6H and says so** rather than returning a verdict it is not entitled to, for
two reasons, both measured over the 21 real assets:

* `decode_bc6` returns 8-bit BGRA, so every value above 1.0 is flattened to 255
  and every value below 0.0 to 0 — it cannot see the range the format exists
  for. It also has no signedness argument, so it can only speak about `UF16`.
* Its `UF16` path is not bit-exact: **20 of 21 assets agree pixel for pixel**,
  and one (`0x0c0000000003e30b`) has **34 differing pixels out of 65,536**,
  every one of them the oracle pinning a channel to 255 where the value is below
  1.0 (largest difference 184). On those pixels our half floats are identical to
  `bcdec`, so the divergence is on the oracle's side.

The weak comparison is still available, for anyone who wants the number rather
than the refusal, and it reports every difference rather than absorbing any:

```
cargo run -q -p ow-tank --bin tex -- --dump-spread 300 --dir D:/ow-dumps/spread
python dev/tex_oracle.py --bc6h --dir D:/ow-dumps/spread     # 20 of 21, exit 1
python dev/tex_oracle.py --bc6h D:/ow-dumps/spread/spread_0c0000000003e30b.dump
```

### What is not verified

* **The `SF16` arithmetic is verified against `bcdec` and the specification, not
  against this install** — no asset on this install stores a signed variant, so
  the `signed: true` path has no real data behind it. It is proven on synthetic
  blocks only.
* `texture2ddecoder`'s BC6H divergence (above) is **counted, not explained**: we
  know our values are the reference ones, not why that decoder saturates.
* **The claim that "the −1 offset does not apply to BC6H" is falsified for these
  files**, but the offset itself is still a rule read off BC7 and the
  uncompressed families, not something the format byte carries.
* A BC6H block whose mode is a reserved number decodes to black at alpha 1.0
  per the specification; that is implemented and unit-tested, but no install
  asset carries one, so it is untested against real data.
* **Not tested:** HDR values above the half range and the interaction with the
  viewer's tone mapping — nothing downstream of the decoder was measured here.

## The skinning Lookup, the shared identity pool, and placed hardpoints (2026-10-18)

The mesh does not name bones: it names slots in the skeleton's Lookup table
(`mskl` remap array), and the table names bones. The reference exporters apply
this remap when writing bone indices (raw → `Lookup[raw]`, past-the-end or a
`0xFFFF` hole meaning "no bone"), and select four-wide spans by element index
(span `n` starts at `n * 4`). `ow_tank::skin` now implements that rule
(`remap_index`, `remap_span`, `blend_span`), and the scene writes joints
through it. What the install says (*install*, Tracer's character model
`0x0d000000000002b1`, 200 bones):

* The table is **not** the identity: `remap[0..8] = [52, 66, 91, 75, 107, 78,
  47, 49]`, 181 entries for 200 bones. Raw slot 1 (2,723 vertices, the neck)
  is bone 66; raw slot 20 (104 vertices) is bone 50. The old path wrote raw
  values as bones, which was invisible in bind pose (every palette entry is
  the identity) and wrong under any pose; the two gate tests that encoded the
  old numbering now resolve through the table.
* All 17,508 weighted vertices map (0 unmapped lanes), and posing the resolved
  bone moves only its own vertices (the gate).
* A mesh may carry several influence spans, but this install's model reader
  surfaces span 0 only; the helpers accept any span length so a wider reader
  needs no new math (proven on synthetic spans, not on file data).

Palette upload: the hero owns the palette tail it computes (base 1 on an
empty scene, appended past map geometry otherwise); map models share one pool
of identity slots at base 1, which is exact in bind pose and keeps every
`u8` joint index valid — 28 skinned map models, 73,975 skinned vertices, 0
unmapped lanes on the sampled chunks. `--no-skin` keeps the identity default.

Two convention terms from the reference exporters are **not** applied, on
purpose: the vertex `(x, −z, y)` shuffle and the UV `(u, 1−v)` flip are that
exporter's output convention, while this pipeline uses file space throughout
(model bytes, placement matrices and hardpoints alike), so applying them would
mirror the scene, not fix it. Unapplied and recorded: the submesh
`BoneIdOffset` the reference adds to each raw index before the Lookup, which
the model reader does not surface — raws map directly, and a mesh whose
offset is non-zero would misbind silently.

Hardpoints place without simulation: `ow_tank::hardpoint` resolves a hardpoint
GUID to a bone by its low 32 bits against the skeleton ids, composes the
local rotation-then-translation frame under the bone's world matrix
(column-major parent-on-the-left, the reference's row-vector order flipped for
the convention, not a different transform), and decomposes to
position/rotation/scale. Measured on Tracer's view model (`0x0d...f1`, 79
bones, 44 hardpoints in its own `MSTU`): every GUID resolves, scales come back
1.0, and the first pair lands at (±0.440, 0.928, −0.016) — a symmetric
shoulder-height pair, the weapon mounts. No hero look sampled (0–60, plus 200
look assets) carries the look-side array, so that path is read but untested
against real data; no sampled model (0 of 291) carries an `MHRP` chunk.

# Camera

The camera is three separable things in this client: a **config-var surface**
that names every tunable, a **first-person camera path** that builds its own
projection and applies its own matrix, and a **world/3P camera path** whose
per-frame interpolation code is *not* in the captured pages. This file records
what is read, what is inferred, and where the capture stops.

All current-build addresses are v2b addresses in the Ghidra program
`module_v21_clean.bin` (build **2.25.0.1.154504**, base `0x7ff7c4d20000`, the
21 October captures). The complete image of the same build is the program
`Overwatch.exe` (preferred base `0x140000000`; read a v2b address there as
`0x140000000 + (v2b - 0x7ff7c4d20000)`). Old-build addresses are in
`module_v2full.bin` (**2.25.0.0.154088**). The two builds do not share
addresses.

Evidence classes used below: **read** = seen in decompiled/disassembled code
or in the image's own bytes; **inferred** = a conclusion from several reads,
not a single instruction; **open/cold** = the page or the reader is not in the
capture. No fitted camera values are introduced here.

## Current state

- **Var surface: mapped in the current build.** Every name record and
  registration stub is located; the resolvable static objects and defaults are
  read. They match the old build's documented defaults, so they are true
  statics, not session-specific data.
- **World near/far: settled.** The world pair is **0.1 / 16384.0 m**;
  `DefaultNearZ`/`DefaultFarZ` are settings vars, and the first-person pair is
  exactly **one tenth** of them (`0.01 / 1638.4`). This closes the near/far
  half of open question 6. The perspective-matrix builder is found (section 4);
  its x double negative (view `A x B`, projection `-x`) cancels.
- **World/FP FOV: read at 103 deg horizontal -> 70.53 deg vertical (16:9).**
  `GetHorizontalFov` reads the 103 setting, and the first-person conversion
  `FUN_7ff7c5700f50` applies `tan`/`atan` with the 9/16 factor to get 70.53.
  `ApplyFirstPersonCamera` sets that on the world camera; the camera
  component's 70.0 deg is the rounded 16:9 fallback. Section 4.
- **First-person camera: path found.** The FP setup is v2b
  `0x7ff7c568c5b0` (read, but not a defined Ghidra function). It sets a
  **60.0 deg initial plane angle**, the FP near/far pair, an animation-blended
  FOV and the camera pose. The pose comes from the FP animation entity
  (`UpdateFirstPersonCameraPose`), not from the world eye. The type-`0x35`
  component consumer (`m_position`, the four scalars) is still unread: the FP
  setup is not that consumer.
- **Per-frame interpolation core: not located in the warm code.** The
  variables `look/position/offset/fov/rollInterpolationSpeedInverse` and the
  `roll/move/look/FOVSpeed` rates have **no direct read sites outside their
  registration stubs** in the current image. The code pages are warm (e.g.
  `0x7ff7c5301000`: 3733/4096 nonzero, entropy 6.55), so this is an
  indirect-dispatch or copied-settings gap, not a missing code page. The 488
  `.text` pages missing from the Ghidra image were filled from
  `module_v21d.bin` and analysed (2026-10-23); the only warm functions that
  read the accumulator offsets are the input tick and a large render update,
  not a look application. What is
  readable is the mode-transition/animation-state code, not the steady tick.
- **Camera shake: data only.** `CameraShake` is an entry in an animation/effect
  name table, not a component class; it has no code references. The local
  hit-effect tunables are read; the shake math is cold.
- **Camera modes: data/statescript.** No first/third/spectator/replay camera
  state machine is readable; only the input-side `ForceInputMode` override and
  the replay/observer view-target chooser are code-visible.

## 1. The config-var surface

The registry mechanism is the one described in
[`input-camera-audio.md`](input-camera-audio.md): a guard byte, an in-place
XOR decode of a name record, string wrappers, a register call with the address
of the static var object, then the guard is set. The current build's records
have the shape `{u8 key[8]; u32 length; char text[length]; pad to 8}`; the
top byte of the length word is the "still encoded" flag and each text byte is
XORed with `key[i & 7]`. The decode sites use a 32-bit displacement from the
image base (e.g. `0x384xxxx`), which is why reference scans against the decoded
text alone miss most of them.

The current-build registration groups and record blocks are:

| group | code | record block | vars |
|---|---|---|---|
| camera / 3P animation | `0x7ff7c52ba018` band (inside `FUN_7ff7c52a9656`) | `0x7ff7c856b7c0..0x7ff7c856ca98` | 40: `camera.*`, `camera3Panim.*`, `camera.3PPositionOffsetY.*`, `camera.3PDistanceSmoothing.*` |
| first-person overrides | `FUN_7ff7c53996c0` (band `0x7ff7c5399700..0x7ff7c539a6a2`) | `0x7ff7c8573e50..0x7ff7c8574438` | 16: `firstPerson.*` |
| local hit effects | `0x7ff7c539fa22` band | `0x7ff7c85744f8..0x7ff7c85746d2` | 5: `localhiteffects.*` |
| FP clip planes | `FUN_7ff7c568c290` | `0x7ff7c8582b10..0x7ff7c8582bac` | `camera.FirstPersonNearZ`, `camera.FirstPersonFarZ` |
| input | `FUN_7ff7c532ada0` | `0x7ff7c856f524..0x7ff7c856f6bc` | 6: `input.*` |
| world clip planes / FOV | `0x7ff7c51e2980` (near, undefined), `FUN_7ff7c51e2890` (far), `FUN_7ff7c51dd630` (FOV) | `0x7ff7c86caab8..0x7ff7c86caae8`, `0x7ff7c86c8dfc..0x7ff7c86c8e0c` | `DefaultNearZ`, `DefaultFarZ`, `HorizontalFOV` |

The register entry is type-specific (`FUN_7ff7c694a030` for float vars,
`0x7ff7c694bfd0` for bools, `0x7ff7c69480b0` for enums, and so on). The
immediate values in a stub are **bounds** (100.0, 360.0, 0), not defaults.
Defaults live in the static objects; a value the stub writes with a plain
store is a default only when the object is read by the stub before
registration.

Function extents in this image are unreliable: `.pdata` is chunked and Ghidra
truncates bodies at cold bytes. A "function" whose recorded size is a few
bytes may own thousands of bytes of body; a registration band can sit inside a
larger init function. Treat the addresses above as **entry points or in-function
sites**, as labelled, not as exact extents.

### Camera block defaults (read unless marked inferred)

| variable | static object | default | note |
|---|---|---|---|
| `camera.gamepadTriggerPower` | `0x7ff7c8584eb8` | 2.0 | |
| `camera.gamePadCameraDampening` | `0x7ff7c8584eb4` | 4.0 (inferred cell) | |
| `camera.gamepadStickPower` | `0x7ff7c8584eb0` | 0.4635 (inferred cell) | |
| `camera.lookInterpolationSpeedInverse` | `0x7ff7c8584ec0` | 0.1 | matches old build |
| `camera.offsetInterpolationSpeedInverse` | `0x7ff7c8584ec4` | 0.1 | matches old build |
| `camera.rollInterpolationSpeedInverse` | `0x7ff7c8584ec8` | 0.1 (inferred cell) | matches old build |
| `camera.rollSpeed` | `0x7ff7c8584ecc` | 1.0 (inferred cell) | matches old build |
| `camera.moveSpeed` | `0x7ff7c8584ed0` | 0.1 | matches old build |
| `camera.lookSpeed` | `0x7ff7c8584ed4` | 1.0 | |
| `camera.FOVSpeed` | `0x7ff7c8584ed8` (inferred) | 1.0 (inferred cell) | not directly decoded |
| `camera.positionInterpolationSpeedInverse` | `0x7ff7c88c42c8` | 0.0 | matches old build |
| `camera.fovInterpolationSpeedInverse` | `0x7ff7c88c42cc` | 0.0 | matches old build |

`camera.EnableUpdate` registers successfully but its static object and default
were not resolved. `camera.forceInput` registers object `0x7ff7c88c42c2`;
`camera.useGamePadCameraDampening` registers object `0x7ff7c88c42c1`; both are
registration-only in the captured pages. The three
`camera.FirstPersonAnimation*` vars are registered (name records
`0x7ff7c856b814`, `0x7ff7c856b894`, `0x7ff7c856b914`) with no read sites in
the capture; the old build's `1.0 / 1.0 / 0` defaults are the only values we
have, and they are not promoted to current-build defaults.

The `camera3Panim.*` block (`0x7ff7c856b97c..0x7ff7c856be7c`) is the authored
3P camera-animation intensity override surface: `enabled`, `overrides.enabled`,
`overrides.all`, `.position`, `.x`, `.y`, `.z`, `.orientation`, `.yaw`,
`.pitch`, `.roll`. It is **not** camera shake.

### firstPerson block

All 16 vars are registered by `FUN_7ff7c53996c0`. Only the offset/debug part
is actually applied:

| variable | object | consumer |
|---|---|---|
| `firstPerson.OffsetX/Y/Z` | `0x7ff7c88a7b24/28/2c` | `FUN_7ff7c539ddd5` |
| `firstPerson.OffsetYaw/Pitch/Roll` | `0x7ff7c88a7b30/34/38` | `FUN_7ff7c539ddd5` |
| `firstPerson.DetachModelFlag` | `0x7ff7c88a7b20` | `FUN_7ff7c539ddd5` (gate) and `0x7ff7c5398840` |
| `firstPerson.WeaponviewOffset*` (six) | `0x7ff7c88a7b3c..50` | no reader in the capture |
| `firstPerson.ModelGuid` | `0x7ff7c8573e4f` | no reader in the capture |
| `firstPerson.ShowFrameFlag` | `0x7ff7c88a7b54` | no reader in the capture |
| `firstPerson.DrawThirdPersonModel` | `0x7ff7c88a7b58` | no reader in the capture |

`FUN_7ff7c539ddd5` reads the offsets, converts yaw/pitch/roll from degrees to
radians (the constant at `0x7ff7c7ca44b8` is pi/180), builds a translation +
rotation transform and combines it with the first-person animation node. It
feeds the **first-person model/view-model transform, not the world camera**.
The `WeaponviewOffset*`, `ModelGuid`, `ShowFrameFlag` and `DrawThirdPersonModel`
vars are registered but have no direct read in the warm image; a read through
the config registry handle cannot be excluded.

### localhiteffects block

`localhiteffects.reticle`, `.edge`, `.health`, `.camera`, `.3PcameraShake` are
registered by the `0x7ff7c539fa22` band. The descriptions are explicit:

- `localhiteffects.camera` — "render camera hit effects" (static default byte
  `1` at `0x7ff7c85744af`);
- `localhiteffects.3PcameraShake` — "render camera shake hit effects in 3P"
  (static presets at `0x7ff7c88a7d10`, see section 7);
- `localhiteffects.health` — default byte `0` at `0x7ff7c85744ae`.

The names are 1P/3P hit-feedback gates. The code that tests them is not in the
capture (the var is reached through the registry, not a direct global read).

## 2. The per-frame interpolation core (open)

The camera tunables that would define the steady tick are present as data but
have **no per-frame reader** in the captured pages:

- `look/position/offset/fov/rollInterpolationSpeedInverse`;
- `rollSpeed`, `moveSpeed`, `lookSpeed`, `FOVSpeed`;
- `camera.EnableUpdate`, `camera.forceInput`;
- the gamepad dampening/stick/trigger vars;
- the 3P ascending/descending distances and chase rates.

Every direct reference to them in the readable `.text` is a registration-site
reference. `live_main_flat.bin` is byte-identical in this region, so it adds
no coverage. They are either read through a registry handle whose code page is
cold, or the consumer is outside the 2.25.0.1 capture.

What **is** readable is the transition-time camera-animation block:

- `FUN_7ff7c53dbedb`, `FUN_7ff7c53dc790`, `FUN_7ff7c53dd6e1` call
  `0x7ff7c52bc350` when the camera mode (`[camera+0xc4]`) changes to 2/3. They
  clear the target, set `[camera+0x14c] = -1`, `[camera+0x148] = 0xa` on stop,
  and resolve the state through `FUN_7ff7c6972420(world,
  0x7ff7c88a09d0)`.
- `0x7ff7c52bc350` rebuilds/applies the camera animation state: it sets the 3P
  distance-smoothing rate (`+0x230 = 30.0`) and active flag (`+0x234 = 1`),
  evaluates `camera3Panim.intensity.enabled`, writes the animation offset
  vector at `+0x100..0x118`, clamps three 3P floats at `+0x180..0x188` to
  `[0, 1]`, and calls the animation graph. Its tail is cold.
- The distance stage at `0x7ff7c52bfa4c` computes a squared distance, an
  rsqrt with one Newton step, compares it against a threshold and returns a
  boolean. It is a target/visibility check, not the interpolation.

A partial camera-animation state layout is read from those functions:
`+0x100..0x118` (animation offset/rotation floats), `+0x180/+0x184/+0x188`
(3P channel/distance values), `+0x230` (smoothing rate), `+0x234` (active).
The camera component's own mode/target fields are `+0xc4` (mode, 2/3),
`+0x148` (state, `0xa` on stop), `+0x14c` (target handle, `-1` = none),
`+0x150/+0x158` (cached target pointers).

**Do not infer an interpolation formula from the inverse-speed defaults.** The
old viewer's `--cam-look-inverse 0.1` filter and any zero-rest spring are fits;
the client's form remains unread. The current fit stays in
[`../fitted-ledger.md`](../fitted-ledger.md).

## 3. First-person camera

### Setup (read)

The FP camera setup is v2b `0x7ff7c568c5b0` (RVA `0x96c5b0`). It is real,
readable code but **not a defined Ghidra function and has no recorded caller**,
which is why its variables looked unread. The shape:

- frame guard: compares the frame counter in `[context+0x2ab4]` with
  `[0x7ff7c8e607d8]` and returns early for the same frame;
- takes a float (XMM1) and a flag (R8B); the float is the animation time;
- walks **six 0x110-stride slots** from `[context+0x2ad0]` and calls
  `0x7ff7c568eab0(slot, time)` on each;
- `0x7ff7c568eab0` dispatches by `round(time * 1000)`: it advances the slot by
  `0x110` and jumps through `[slot_vtable+0x200]`. The constant at
  `0x7ff7c7ca0e60` is `1000.0`; the bias is `0.5`. This is an animation-event
  dispatch, not a camera write;
- the projection is a three-value description applied through
  `FUN_7ff7c529a140(camera, index, value)`: index 0 = **60.0 deg** converted
  to radians, index 1 = the FP near value, index 2 = the FP far value. The
  setter returns the previous value;
- the 60.0 deg constant is at `0x7ff7c8582b0c`, read at `0x7ff7c568c980`.
  It has no name record of its own. Its axis (horizontal or vertical) is not
  established. It is **not** the per-hero `STUFirstPersonComponent` scalar 50;
- the animation FOV blend is read as
  `v = old + ((2.0 * [anim+0xd8]) - old) * [anim+0x68]` (constants
  `2.0` at `0x7ff7c7ca0ffc`, the old plane value returned by the setter), then
  applied through `0x7ff7c6d8b4a0`;
- the previous near/far values are cached at `[camera+0x254]` and
  `[camera+0x258]` when they change;
- the camera's own 4x4 matrix `+0x10..0x40` is copied to the stack and passed
  to `[camera_vtable+8]`;
- the frame counter is stamped into `[context+0x2ab4]`.

The old-build equivalent is v2b `0x7ff7c56b2770` (same logic; setter
`0x7ff7c52a12a0`; near/far registration `0x7ff7c56b2450`). The old-build
near/far reference at `0x7ff7c56b252e` from `input-camera-audio.md` resolves
into this setup, not into a separate function.

### The component path (still open)

`STUFirstPersonComponent` is data (class hash `0x9d28963f` -> type index
`0x35`; descriptor `0x7ff7c8c9c640` in the old build). The runtime layout is
read from the old session: `m_position` at `+0xc0`
(`0.1375, 0.23375, 0.20625`), the `64/16/-1` drag spring at `+0xf0`, the four
scalars at `+0x100` (`50, 1.25, 0.875, 0.8`).

The known consumer `FpViewmodelConsumer` (`0x7ff7c5a4f060` ->
`FpViewmodelBind` `0x7ff7c5a4faa0`) stores references and never reads
`m_position` or the four scalars. The per-frame slots `0x7ff7c5a51bf0` /
`0x7ff7c5a522f0` walk the six animation slots and run the event player. **No
warm function reads `+0xc0/+0xf0/+0x100` in a camera path.** The type-`0x35`
camera consumer is not in this capture; open question 2 remains open. The
`50` scalar is a candidate but is **not** the FP camera's own 60.0 deg angle.

## 4. World camera and projection

### Near/far (read)

| var | name record | static object | default |
|---|---|---|---|
| `DefaultNearZ` | `0x7ff7c86caab8` (text `0x7ff7c86caac4`) | `0x7ff7c8e5c370` | 0.1 |
| `DefaultFarZ` | `0x7ff7c86caad8` (text `0x7ff7c86caae4`) | `0x7ff7c8e5c3e0` | 16384.0 |

The registration functions are `0x7ff7c51e2980` (near; real entry, not yet a
Ghidra function) and `FUN_7ff7c51e2890` (far). The getters are
`FUN_7ff7c686cbc0` (near) and `FUN_7ff7c686cba0` (far); each falls back to
0.1 / 16384.0 when the settings var is disabled. Both register through the
settings dispatcher `FUN_7ff7c6978170`.

Four view/camera constructors copy the getter results at construction:

| constructor | near/far offsets |
|---|---|
| `FUN_7ff7c6d89710` | `+0x254/+0x258` |
| `FUN_7ff7c6b4ce60` | `+0x5c8/+0x5cc` |
| `FUN_7ff7c6b78760` | `+0x118/+0x11c` |
| `FUN_7ff7c6a961d0` | `+0x70/+0x74` |

The first-person pair is exactly **one tenth** of the world pair. Startup
shims multiply the settings values by 0.1:

- `FUN_7ff7c4d84a50` writes `0.1 * DefaultNearZ` -> `0x7ff7c88bc8ac` (0.01);
- `FUN_7ff7c4d84a30` writes `0.1 * DefaultFarZ` -> `0x7ff7c88bc8b0` (1638.4);
- `FUN_7ff7c568c290` then registers `camera.FirstPersonNearZ/FarZ` with those
  objects and clamps them between `0.1 * DefaultNearZ` and `DefaultFarZ`.

The old build confirms the same numbers: a getter pair at
`0x7ff7c68b5270/0x7ff7c68b5290`, the same 0.1 scale, and the FP default pair
`0.01f / 1638.4f` at `0x7ff7c89e6860`. The decoded descriptions call the FP
pair a "near/far clipping plane override for objects rendered in first person
mode" (`0x7ff7c8582b4c` / `0x7ff7c8582bcc`), i.e. a **render-mode override**,
not a separate camera mode.

**Consequence for the viewer:** the world pass now uses the read `0.1 / 16384`
pair, and the FP pass uses `0.01 / 1638.4`; the old `0.01 / 8000` world
stand-in and its ledger row (`FIT-CAM-04`) are removed.

### Horizontal FOV

`HorizontalFOV` is a settings variable, not a `camera.*` var. In the current
build the name text is at `0x7ff7c86c8e0c`; the writer is
`FUN_7ff7c51dd630`, which registers var object `0x7ff7c8e59ec0` with
`+0x38 = +0x3c = 103.0`, `+0x40 = 70.0`, `+0x44 = 103.0`, `+0x48 = 1`
(settings record: value, default, min, max, enabled; the field meaning is
inferred from the identical shape of `DefaultNearZ/FarZ`). The old-build name
is `0x7ff7c87f2f9c`, written at `0x7ff7c51dd6a8` to `0x7ff7c8f83db8`.

**`HorizontalFOV` has a reader: `GetHorizontalFov` (`0x7ff7c6850fc0`).** The
old "no reader" conclusion was wrong because the obfuscated getter was not
followed. Its callers are `FUN_7ff7c684b940` (aspect-ratio FOV correction),
`FUN_7ff7c684bc20` (a wrapper) and `FUN_7ff7c684ec50` (a settings snapshot,
which stores the value at `+0xd`). The first-person FOV path uses it:
`UpdateFirstPersonCameraPose` (`0x7ff7c56b7b90`) calls
`FUN_7ff7c5700f50(0)`, which calls `FUN_7ff7c684b940`; that function starts
with the 16:9 factor **9/16 = 0.5625**, corrects it with the live viewport
aspect, and returns the horizontal angle. `FUN_7ff7c5700f50` then:

1. multiplies 103 by **0.00872665** (half of deg2rad, on-disk `.rdata`
   `0x7ff7c7cb3da8`) -> 0.8988 rad half-angle;
2. `tanf` -> 1.257;
3. multiplies by **9/16** -> 0.7071;
4. `atanf` -> 0.6155 rad; doubles -> **1.231 rad = 70.53 deg**.

The half-angle constant is now present in the Ghidra program (the 2026-10-23
page fill), so the decompiler shows the real operand instead of the zero
`.rdata` page.

So the first-person vertical FOV is **70.53 deg at 16:9**, the same as the
world's 103-horizontal setting converted to vertical; the 60.0 deg angle is a
fallback, and the per-hero `50` scalar is not the FOV.
`ApplyFirstPersonCamera` sets this pose FOV on the world camera each frame.

The gameplay camera component carries **70.0 deg vertical** (`0x3f9c61aa`) at
`+0x48`; `UpdateCameraComponent` (`0x7ff7c6dc4bb0`) reads it and passes it to
`SetVerticalFovFull`. `SetCameraFov` (`0x7ff7c6dc50c0`) also writes `+0x48`,
but it has **no references and no vtable entry** in the image.
`InitCameraViewParams` (`0x7ff7c6d89710`) stores the same FOV halved (35.0 deg,
`0x3f1c61aa`) at view `+0x224`. The **70.0 deg value is the rounded 16:9
fallback**; the live first-person path sets **70.53 deg** (103 horizontal)
through `ApplyFirstPersonCamera`. The 0.55 deg difference is the rounding.

### Projection matrices

`BuildPerspectiveMatrix` (`0x7ff7c6d95a70`) and `SetPerspectiveProjection`
(`0x7ff7c6d95630`) are in the warm code. The builder stores row-major floats
and is used with row-vector `v*M`. `BuildViewMatricesFromCameraDescriptor`
(`0x7ff7c6d8afe0`) writes camera-to-world rows `[A x B; B; A; e]`, where `A`
is the descriptor's forward row (`+0x10`) and `B` its up row (`+0x20`); for
`A=+Z`, `B=+Y`, `A x B = -X`. The internal `SetPerspectiveProjection` passes
the standard-depth branch (`0`), so clip depth is 0..1 (`m[10]=f/(f-n)`,
`m[11]=1`, `m[14]=-fn/(f-n)`); the export helper `FUN_7ff7c6d95440` passes `1`
for a reversed-Z variant.

The vertical scale is `m11 = 1/tan(halfV)`. The x scale is read as
`m00 = -E/tanH` with `E=+1.0` at camera `+0x228`, while `tanH` is derived from
`tanV` and the view rect. The view's x row `A x B` is **negative** x and the
projection's x scale is negative, so the two signs cancel and the image is
right-positive; the masks at `0x7ff7c7facd40`/`0x7ff7c7fad130` are zero /
identity in the flattened image. The viewer's right-handed
`look_to_rh` + `perspective_rh` pair produces the same image; it takes the read
near/far values and the read 103-horizontal / 70.53-vertical world FOV.
`FUN_7ff7c6d89710` is a 0x270-byte view struct with vtable
`0x7ff7c7caa148`, matrices at `+0x10` and `+0x50`, a sub-object at `+0x90`,
the 35 deg half-angle at `+0x224`, and near/far at `+0x254/+0x258`.

## 5. Input to camera

### Registry and polling (read)

`FUN_7ff7c532ada0` registers the six `input.*` vars. The static objects are:

| var | object |
|---|---|
| `input.MouseScaleX` | `0x7ff7c856ec1c` |
| `input.MouseScaleY` | `0x7ff7c856ec34` |
| `input.GamePadAimAssistEnable` | `0x7ff7c856ec04` (bool) |
| `input.GamePadRenderDebug` | `0x7ff7c88a376c` (bool, no reader) |
| `input.ForceInputMode` | `0x7ff7c856ec4c` (enum) |
| `input.ShowPhysicalButtonBindings` | `0x7ff7c857bed0` (bool) |

The per-frame device poll is `0x7ff7c5329100..0x7ff7c53296e7`. It gates on
`[input+0x229]`, reads the gamepad aim-assist flag, takes the raw look counts
from a device vtable call at `+0x330`, converts the timestamp to double
seconds at `[input+0x2230]`, converts both counts to floats at
`[input+0x2224]/[+0x2228]`, multiplies them by `MouseScaleX`/`MouseScaleY`
(the only reads of those vars in the image), and writes them back.

The look-delta producer is `FUN_7ff7c54ba450`: it scales by
`[input+0x2238] * K`, emits `yaw = -scale * dx`, `pitch = +/-scale * dy`
(sign flipped when `[input+0x2156]` is set). `K` at `0x7ff7c7ccc4b0` is in
`.rdata` (RVA `0x2fac4b0`), not `.text`. The merged image zeroed that `.rdata`
page, but the on-disk `Overwatch.exe` carries `0x3C3CBADB` = **0.011519**
rad/count at the same RVA (the Ghidra program now contains the filled page).
The default input scale is `0.15` (old-build value), giving **0.00172785**
rad/count (0.099 deg/count). A second variant
`FUN_7ff7c54bb900`
adds an aim-assist delta from the view-target solve `0x7ff7c56da800` before
the same transform.

The command-frame builder at `0x7ff7c5301ab5` calls `FUN_7ff7c54ba450` with
output yaw at `+0x1294` and pitch at `+0x1298`; it adds secondary aim deltas
at `+0x129c/+0x12a0`, sets input bytes `+0x122e/+0x122f/+0x1234` and the
button/flag word `+0x1230`, copies `+0x1250/+0x1252` to `+0x1228/+0x122a`
and the vec4s at `+0x1260/+0x1270`, then hands the frame to
`0x7ff7c56b7060` and `0x7ff7c53035c0`/`0x7ff7c5303cc0`. The handoff into the
world camera is not located in the warm code (the code pages are nonzero;
the consumer is indirect or reads a copied block). No clamp of yaw/pitch is
present in the captured window. The camera component stores read pitch limits
of **-89/+89 deg** at `+0x94`/`+0x98` (`ConstructCameraComponent`
`0x7ff7c6dc3340`, from the int globals `0x7ff7c87c0e70`/`0x7ff7c87c0e74`),
but the look integrator that consumes them is not located, so applying those
limits to look is not established.

**Correction:** the `"commandframe"` string at `0x7ff7c7ca9288` is a **replay
stream name**, not the movement command frame. It sits in the replay stream
block with `format`, `chunked`, `fastforward`, `target`, `loop`, `live`.

### Camera mode override and spectator

`input.ForceInputMode` is read at `0x7ff7c532a913` (inside
`FUN_7ff7c532a8a4`); a value of `-1` means no override (inferred from the
compare), otherwise it is passed to `0x7ff7c54be7b0`. The per-frame mode read
maps a four-value enum: 0/1 -> game, 3 -> spectate, 2 -> unchanged; the config
description names only 0-Global, 1-Game, 2-Spectate, so the fourth value is
unnamed. `[input+0x2157]` is the spectator gate that feeds the aim-assist
path.

## 6. Camera modes, spectator and replay

There is no camera-mode string vocabulary in the module: no `Spectator`,
`Observer`, `FreeCam`, `DeathCam`, `KillCam`, `ThirdPerson` or `Shoulder`
camera strings. The only mode-adjacent evidence is:

- `input.ForceInputMode` ("Force input mode 0-Global, 1-Game, 2-Spectate") and
  the four-value enum above;
- `maxObservers` (`0x7ff7c83117ec`), an unreferenced lobby setting;
- the replay telemetry message `Blizzard.Telemetry.Pro.Client.Replay.ReplayCameraView`
  (pb path `0x7ff7c7e64e50`, typename `0x7ff7c7e64ec0`): constructor
  `0x7ff7c5c64e60`, `MergeFrom` `0x7ff7c5c64ed0`, with a presence mask at
  `+0x30` (bits 0-5), `int32` at `+0x40`, floats at `+0x44/+0x48/+0x4c`,
  strings at `+0x38/+0x50`. It is a telemetry event only; its producer is not
  in the warm code;
- the replay/observer view-target chooser at `0x7ff7c53d2bb2..0x7ff7c53d31b4`
  (string "Found preferred viewtarget %s: %s" at `0x7ff7c7ca93f8`). It builds
  a candidate name from `[object+0xd8]+0x2b0`, hashes it, walks a candidate
  list by prefix and keeps the longest match. Obfuscated decoder blocks are
  interleaved; the callers are indirect.

A first/third/spectator camera state machine is therefore **not readable** in
this capture. Mode selection is data/statescript-driven; only the input-side
override is code-visible.

## 7. Camera shake and hit feedback

`CameraShake` is **not a component class**. Its record is in the 64-bit
name->hash table at `0x7ff7c86cb040` (key `0xe5cc99d7855028de`, text
`0x7ff7c86cb04c`), next to `Impulse`, `MuzzleFlash`, `DepthOfField`,
`MotionBlur`, `BlendAnim`, `LensFlare`. The key occurs exactly once in the
whole image and the record/string have **zero code references** in both builds
(the old-build table is a different build key at `0x7ff7c87f504c`). The table
is inert `.rdata` - an animation/effect name dictionary, not the runtime class
registry. Do not use `Camera`/`CameraShake` entries to find C++ classes.

The tunable surface is `localhiteffects.*` (section 1). The 3P shake config
objects are at `0x7ff7c88a7d50` and `0x7ff7c88a7e90` (two 0x140-byte records,
registry ids `0x4c` and `0x4d`; 1P/3P pairing inferred); their defaults are
copied by `FUN_7ff7c4d667e0` from `.rdata 0x7ff7c85744b0/c0/d8/e0/f0` into
`0x7ff7c88a7d10`. Three 20-byte presets read **(0.35, 0.28, 0.35),
(0.32, 0.25, 0.32), (0.30, 0.23, 0.30)** with per-record flags. The roles of
the three floats (amplitude/frequency/decay candidates) are **not**
established, and there is no oscillation, decay, accumulator or camera-offset
write attributable to `CameraShake` in the readable pages. The registration
thunks are `0x7ff7c4d667a0` and `0x7ff7c4d667e0`.

Weapon recoil is separate: `animRecoilImpulseFactor` in the weapon graphs is
an animation/weapon parameter, not this camera shake. The shake math needs a
capture with the effect-node dispatch pages warm.

## 8. Function names applied to the Ghidra image

The camera entries below were named in `module_v21_clean.bin`; on 2026-10-24
the names were migrated to `Overwatch.exe` (address `0x140000000 + RVA`) and
that program was deleted. The current `docs/function-names.csv` has 174 named
rows. The names are the ones this document uses; `dev/gh_apply_names.py`
applied them and saved the program.

| address | name | role |
|---|---|---|
| `0x7ff7c686cba0` | `GetDefaultFarZ` | reads `DefaultFarZ`, fallback 16384.0 |
| `0x7ff7c686cbc0` | `GetDefaultNearZ` | reads `DefaultNearZ`, fallback 0.1 |
| `0x7ff7c51e2890` | `RegisterDefaultFarZ` | registers `DefaultFarZ` (16384/100/20000) |
| `0x7ff7c51dd630` | `RegisterHorizontalFov` | registers `HorizontalFOV` (103/70/103) |
| `0x7ff7c529a140` | `SetCameraPlaneValue` | (camera, index, value) -> previous; 0=angle, 1=near, 2=far |
| `0x7ff7c54ba450` | `ComputeMouseLookDelta` | scale counts, yaw = -s*dx, pitch = +/-s*dy |
| `0x7ff7c54bb900` | `ComputeAimAssistedLookDelta` | same plus view-target aim delta |
| `0x7ff7c568c290` | `RegisterFirstPersonClipPlanes` | registers `FirstPersonNearZ/FarZ` |
| `0x7ff7c568eab0` | `DispatchFpCameraSlotTime` | `round(t*1000)` -> six-slot event dispatch |
| `0x7ff7c539ddd5` | `ApplyFirstPersonOffsetTransform` | degrees->radians FP model offset transform |
| `0x7ff7c5329100` | `InputPollMouseAndGamepad` | raw look counts, timestamp, mouse scale |
| `0x7ff7c532a8a4` | `ApplyForceInputMode` | `ForceInputMode` override -> input mode |
| `0x7ff7c56b7640` | `ConstructFirstPersonCamera` | FP camera: vtable, animation/view-model refs, 70 deg default FOV |
| `0x7ff7c56b7b90` | `UpdateFirstPersonCameraPose` | FP pose from animation basis/position, pose FOV |
| `0x7ff7c56b7c90` | `BuildFirstPersonCameraVariant` | constructs the FP camera variant, vtable `0x7ff7c7d79448` |
| `0x7ff7c57d3110` | `PlayerCameraControllerUpdate` | reads the player camera component, sets mode and view pointer; not the look path |
| `0x7ff7c5c64ed0` | `ReplayCameraViewMergeFrom` | generated replay telemetry proto |
| `0x7ff7c6d89710` | `InitCameraViewParams` | view struct: vtable, matrices, 35 deg half-FOV, near/far |
| `0x7ff7c6d89a20` | `ValidateAndNormalizeCameraDescriptor` | normalizes basis, orthogonality check, FOV clamp `[0.05, 3.0]` |
| `0x7ff7c6d8afe0` | `BuildViewMatricesFromCameraDescriptor` | descriptor -> camera-to-world and world-to-view matrices |
| `0x7ff7c6d8b4a0` | `SetVerticalFovFull` | full vertical FOV -> half angle `+0x224`, rebuild projection |
| `0x7ff7c6d8db20` | `UpdateCameraView` | per-frame base view: transition, component, modifiers, descriptor |
| `0x7ff7c6d95630` | `SetPerspectiveProjection` | projection from view rect and half FOVs |
| `0x7ff7c6d95a70` | `BuildPerspectiveMatrix` | row-major left-handed 0..1 perspective matrix |
| `0x7ff7c6dc3340` | `ConstructCameraComponent` | component defaults: 70 deg FOV, -89/+89 deg pitch limits, near/far |
| `0x7ff7c6dc4bb0` | `UpdateCameraComponent` | component FOV, basis, offset filter, validation |

These roles are read but **not** named, because their entry point is not a
function in Ghidra or the role is still inferred:

| address | candidate | why not applied |
|---|---|---|
| `0x7ff7c51e2980` | `RegisterDefaultNearZ` | real entry, not a defined function |
| `0x7ff7c568c5b0` | `ApplyFirstPersonCamera` | real entry, not a defined function |
| `0x7ff7c5301ab5` | `BuildLocalCommandFrame` | real entry, not a defined function |
| `0x7ff7c53d2bb2` | `SelectPreferredViewTarget` | not a function |
| `0x7ff7c52bc350` | `CameraAnimStateRebuild` | function, role inferred only |
| `0x7ff7c4d667a0` | `Register3PCameraShakeConfig` | not a defined function |
| `0x7ff7c4d667e0` | `InitLocalHitEffectShakeDefaults` | not a defined function |


## 9. Dead ends worth recording

- **The 64-bit name table is not a class registry.** It holds animation/effect
  channel names (`TimeScale`, `DisableJoint`, `Camera`, `Sequence`, `Impulse`,
  `CameraShake`, `MuzzleFlash`, ...). It has no code references; using it as
  the Camera component identity is a false lead.
- **`FUN_7ff7c5687b70` / `FUN_7ff7c5687970` are not projection code.** Their
  decoded strings are `success`, `No modal is currently open.`,
  `Unknown event: %s`; `FUN_7ff7c5687970` hashes a `std::string` with FNV-1a
  and appends 0x10/0x18-byte records. They are message/exception helpers. Do
  not name them after the camera.
- **`FUN_7ff7c52be672` is mis-bounded** in Ghidra (overlapping instructions,
  unaffiliated registers). It references camera animation constants but is not
  safe to read as one function; use the bounded code around `0x52bcf61` /
  `0x52bdea0` instead.
- **The `Camera`/`CameraShake` hashes occur only in the table itself.** A
  scan of the whole image found one occurrence of each; there is no immediate
  compare to follow.
- **Function extents are untrustworthy.** `.pdata` is chunked; several
  "functions" have a 35-byte recorded size over thousands of bytes of body,
  and some real entry points (`0x7ff7c568c5b0`, `0x7ff7c51e2980`,
  `0x7ff7c5301ab5`, `0x7ff7c53d2bb2`) are not defined in Ghidra at all.
- **The Ghidra image matches the on-disk current-build image** at
  `RVA 0x384b814` (`camera.FirstPersonAnimationWeight`) and
  `RVA 0xd2f060` (`FpViewmodelConsumer`); a subagent's "Ghidra differs from
  disk" warning was checked and is false.

## 10. What would settle the open parts

1. **Camera interpolation tick.** A capture with the registry-reader pages
   warm, or a live read trace of the five inverse-speed / four rate objects.
   The addresses to watch are the static objects in section 1 and the
   `FUN_7ff7c52a9656` / `0x7ff7c52bc350` family.
2. **First-person type-`0x35` consumer.** Find a warm function that reads
   component `+0xc0/+0xf0/+0x100`; the FP setup at `0x7ff7c568c5b0` is a
   different path. Open question 2.
3. **Perspective matrix builder.** Read the renderer's projection build and
   confirm the 0.1/16384 world pair and the 70/70.53 vertical FOV at the point
   of use; matrix convention is still unknown.
4. **Shake math.** Capture the effect-node dispatch/update pages, then read
   the three presets' roles.
5. **3P camera consumer.** Read the consumer of `camera.3PPositionOffsetY.*`
   and `camera.3PDistanceSmoothing.chaseRate`; the current image has
   registration data only.
6. **Mode selection.** Find the data/statescript path that selects first vs
   third person and spectator; no C++ switch is visible.

## 11. Commands and reproduction

```sh
# data side: the FP component, hero values and class registry
cargo run -p ow-tank --example fpcomp_probe -- 3

# viewer eye-check (the operator's check, not a proof)
cargo run --release -p ow-view -- --play --hero 3 --anim auto
```

Ghidra side: `module_v21_clean.bin`, v2b base `0x7ff7c4d20000`; the camera
var strings start at `0x7ff7c856b7cc`, the registration band at
`0x7ff7c52ba018`, the FP setup at `0x7ff7c568c5b0`, and the world near/far
writers at `0x7ff7c51e2890` / `0x7ff7c51e2980`. Old-build cross-checks use
`module_v2full.bin` at the addresses in `input-camera-audio.md` and
`animation.md`.

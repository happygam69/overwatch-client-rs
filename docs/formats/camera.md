# Camera

The camera is three separable things: a **config-var surface**, a
**first-person camera path**, and a **world/third-person path**. The surfaces
and the projection are read; the per-frame look/interpolation consumer is not.

## Current state

- **Config surface: read.** Every name record and registration stub is
  located; the resolvable static defaults are read.
- **Projection: read.** World near/far, the FP pair, the horizontal→vertical
  FOV conversion and the matrix builders.
- **First-person path: read.** The FP camera constructor, the pose update and
  the animation evaluation.
- **Open:** the per-frame look consumer, the type-`0x35` component consumer,
  the 3P consumer and the shake math.

## Config surface

The registry is a one-shot stub per variable: decode the name record in place,
register the static object, set a guard byte. The camera blocks:

| group | registration |
|---|---|
| camera / 3P animation | per-variable registration stubs; the former group label was not a function entry (**open**) |
| first-person overrides | `RegisterFirstPersonSettings` (`0x1406796c0`) |
| FP clip planes | `RegisterFirstPersonClipPlanes` (`0x14096c290`) |
| world near/far | `RegisterDefaultNearZ` (`0x1404c2980`), `RegisterDefaultFarZ` (`0x1404c2890`) |
| world FOV | `RegisterHorizontalFov` (`0x1404bd630`) |
| input | `RegisterInputSettings` (`0x14060ada0`) |

The camera/3P group's registration body is **open**: the old
`RegisterCameraConfigVariables` row at `0x140599740` was an offcut inside
`FUN_140599730`, not a function entry, so it was removed. The defaults below
are still read from the config table; the per-variable stubs that write them are
not yet anchored.

Read defaults:

| variable | default |
|---|---|
| `camera.lookInterpolationSpeedInverse` | `0.1` |
| `camera.offsetInterpolationSpeedInverse` | `0.1` |
| `camera.moveSpeed` | `0.1` |
| `camera.lookSpeed` | `1.0` |
| `camera.rollSpeed` | `1.0` |
| `camera.positionInterpolationSpeedInverse` | `0.0` |
| `camera.fovInterpolationSpeedInverse` | `0.0` |
| `DefaultNearZ` / `DefaultFarZ` | `0.1` / `16384.0` |
| `camera.FirstPersonNearZ` / `FarZ` | `0.01` / `1638.4` |
| `HorizontalFOV` | `103.0` |
| `camera.FirstPersonAnimationWeight` | `1.0` |
| `camera.FirstPersonAnimationRollWeight` | `1.0` |
| `camera.ShowFirstPersonAnimationOffset` | `0` |

The FP pair is exactly one tenth of the world pair. The `firstPerson.Offset*`
and `WeaponviewOffset*` defaults are `0`. A default not listed here is not
re-read in the current image and is treated as open.

## Projection

`GetDefaultNearZ` (`0x141b4cbc0`) and `GetDefaultFarZ` (`0x141b4cba0`) fall
back to `0.1`/`16384.0` when the settings var is disabled.

`HorizontalFOV` is a settings variable, not a `camera.*` one. `GetHorizontalFov`
(`0x141b30fc0`) reads `103.0`; the FP conversion applies:

1. `103 * 0.00872665` (half of deg2rad) → `0.8988` rad;
2. `tan` → `1.257`;
3. × `9/16` → `0.7071`;
4. `atan` → `0.6155` rad; ×2 → **`1.231` rad = `70.53°`**.

So the FP vertical FOV is the same as the world's 103° horizontal at 16:9.
The component's `70.0°` is the rounded fallback; the per-hero `50` scalar is
not the FOV.

Projection builders:

| function | address | role |
|---|---|---|
| `BuildPerspectiveMatrix` | `0x142075a70` | row-major, left-handed, depth 0..1 |
| `SetPerspectiveProjection` | `0x142075630` | projection from the view rect and half-FOVs |
| `BuildViewMatricesFromCameraDescriptor` | `0x14206afe0` | descriptor → camera-to-world and world-to-view |
| `OrthonormalizeCameraBasis` | `0x142069c60` | basis normalisation (verified by the harness) |
| `ValidateAndNormalizeCameraDescriptor` | `0x142069a20` | orthogonality check and FOV clamp `[0.05, 3.0]` |
| `SetVerticalFovFull` | `0x14206b4a0` | full vertical FOV → half angle, rebuild |
| `RebuildProjection` | `0x14206a4a0` | rebuild the projection |

The view's x row and the projection's negative x scale cancel; the image is
right-positive.

## First-person camera

| function | address | role |
|---|---|---|
| `ConstructFirstPersonCamera` | `0x140997640` | FP camera: vtable, animation/view-model refs, default FOV |
| `ApplyFirstPersonCamera` | `0x14096c5b0` | applies the pose to the world camera |
| `UpdateFirstPersonCameraPose` | `0x140997b90` | builds the pose from the FP animation basis/position and FOV |
| `BuildFirstPersonCameraVariant` | `0x140997c90` | constructs the variant |
| `EvaluateFirstPersonCameraAnimation` | `0x14096cb30` | evaluates the FP camera animation |
| `IsFirstPersonCameraActive` | `0x140997940` | active test |
| `DispatchFpCameraSlotTime` | `0x14096eab0` | `round(t*1000)` → six-slot event dispatch |

`UpdateFirstPersonCameraPose` takes position `anim+0x1350`, forward `+0x1310`,
up `+0x1320` and the 70.53° FOV. The pose is the animated FP entity's pose,
not a fixed eye offset. The view-model component's offset/scalars are applied
by a separate system that is still open.

## Input

| function | address | role |
|---|---|---|
| `InputPollMouseAndGamepad` | `0x140609100` | raw look counts, timestamp, mouse scale |
| `ComputeMouseLookDelta` | `0x14079a450` | scale counts; `yaw = -s*dx`, `pitch = ±s*dy` |
| `ComputeAimAssistedLookDelta` | `0x14079b900` | same plus the view-target aim delta |
| `EvaluateForceInputMode` | `0x14060a570` | `ForceInputMode` override |
| `ApplyInputSettingsToState` | `0x14079ff00` | applies the input settings |

The look-delta producer scales by `[input+0x2238] * K`. `K` is in `.rdata` at
RVA `0x2fac4b0`, `0x3C3CBADB` = **`0.011519` rad/count**. The remaining input
scale and the per-frame handoff to the camera are open.

The camera component stores pitch limits of **-89/+89 deg** at `+0x94`/`+0x98`
(`ConstructCameraComponent`, `0x1420a3340`). The integrator that consumes them
is not located, so the clamp is not applied as client behaviour.

## Camera modes and third person

| function | address | role |
|---|---|---|
| `UpdateCameraSystem` | `0x14203c4e0` | the camera system update |
| `SetCameraMode` / `SelectCameraMode` | `0x1420a50d0` / `0x1420a5410` | mode selection |
| `SetCameraView` | `0x1420a52a0` | view selection |
| `BuildThirdPersonCameraPose` | `0x14206e400` | 3P pose |
| `TraceThirdPersonCamera` | `0x14206e1a0` | 3P collision trace |
| `SolveThirdPersonCameraCollision` | `0x1420b0f40` | 3P collision response |
| `BuildLocalHitEffectEntry` | `0x1409e1b50` | local hit-effect surface |
| `UpdateLocalHitEffectEnable` | `0x1409e20c0` | hit-effect enable |

The 3P consumer, the shake math and the first/third/spectator mode logic are
open. `CameraShake` in the string table is an animation/effect name, not a
component class.

## Open

1. **Per-frame look/interpolation.** Which function reads the five inverse
   speeds and four rates and applies them to the camera?
2. **Type-`0x35` first-person component.** `FpViewmodelConsumer`
   (`0x140d2f060`) and `FpViewmodelBind` (`0x140d2faa0`) store the view-model
   refs but do not read `m_position` or the four scalars.
3. **Pose/spring target.** The FP spring on `+0x1350` toward `+0x1370` has a
   read coefficient set; its target writer is open.
4. **3P and shake.** Read the consumers before implementing them.

The current viewer's camera framing options (`--fp-cam-drop`, `--fp-cam-back`)
are deviations in `docs/deviations.md`, not client behaviour.

# Input, camera, and audio — and the config-var registry they hang off

## Current state

- **Input:** config-var registry and handler names read; the per-frame
  handler update is open. The input->command-frame path (mouse scale, aim
  assist, force-input mode) is now partly read; see `camera.md`.
- **Camera:** the var surface with defaults is read (HorizontalFOV 103, FP
  near/far 0.01/1638.4, interp speeds); the FP projection pass is
  implemented; the world near/far are now read (`0.1/16384`, FP one tenth);
  the per-frame interpolation consumer is open. The full camera map is
  `camera.md`.
- **Audio:** Wwise is statically linked; metadata and WEM containers are
  read; no audio is decoded (needs an encoder; not started).

**Verify:** `cargo run -p ow-tank --example fpcomp_probe -- 3` (FP data);
`cargo run --release -p ow-view -- --play --hero 3` (camera eye-check).


Three subsystems read this round, plus the registry mechanism they share. All
addresses are in the v2b session (the dump that `module_v2full.bin` holds);
range = v2b − `0x17AEC0000`, exec = v2b − `0x71820000`.

## The config-var registry (the mechanism)

Every knob named below is a **config variable**: a static var object plus a
one-shot registration stub. The stub's shape, read twice — by window
disassembly in the Ghidra project and by REA over a minimal PE built from the
capture region (`dev/mkpe.py`) — is:

* guard with a flag byte (e.g. v2b `0x7ff7c89cd66c`);
* **decode the name(s) in place**: a name record is
  `{u8 key[8]; u32 count | flag<<24; char enc[]}`; while the flag bit in the
  length word is set, each byte is XORed with `key[i & 7]` and the flag is
  cleared from the length — the decode-once idiom, here in its exact operator
  form (`enc[i] ^= key[i & 7]`);
* wrap one or two decoded names into string objects;
* call the register entry (`FUN_7ff7c69975b0` in v2b terms; range
  `FUN_7ff64bad75b0`) with both names and the **address of the static var
  object**, then destroy the wrappers;
* set the guard flag.

Two name records per var are common (a short name and a qualified one). The
movement-command var's records sit at v2b `0x7ff7c8697e08` (key) /
`0x7ff7c8697e14` (`"player.GenerateRandomMovement"`) and `0x7ff7c8697e38` /
`0x7ff7c8697e44`; its var object is at v2b `0x7ff7c89cd4e0`, and its stub is
`FUN_7ff7c530cde0`-family (REA: `FUN_7ff64a44cde0`; the exact bytes read at
exec `0x7ff753aece60..0x7ff753aecef1`).

This explains a large family of small functions the analyses had left
unanalyzed: **one registration/accessor stub per config var**, emitted
mechanically, each carrying its own inlined name record.

## Input

| anchor (v2b) | kind |
|---|---|
| `0x7ff7c8699524` `input.MouseScaleX`, `0x7ff7c8699564` `input.MouseScaleY` | var names, one registration stub (exec `0x7ff753b1c4ee`, v2b `0x7ff7c533c4ee`) |
| `0x7ff7c86995a4` `input.GamePadAimAssistEnable` | var name |
| `0x7ff7c86995fc` `input.GamePadRenderDebug` | var name (gamepad debug draw) |
| `0x7ff7c8699654` `input.ForceInputMode` | var name, same stub |
| `0x7ff7c86a60dc` `ECSingletonInput_C WorkingBindings Input Handler` | input-handler class name; referenced at v2b `0x7ff7c54d6059` |
| `0x7ff7c87f2b74` `teScopedTemporaryInputHandler` | scoped handler name; referenced at v2b `0x7ff7c688efc9` |
| `0x7ff7c7d2a338` `"commandframe"` | a replay stream name, not the movement command frame (corrected 2026-10-23) |

The input surface the earlier rounds needed for the mover is all here: mouse
scale, gamepad aim-assist and debug, forced input mode, the two handler class
names, and the command frame. `player.GenerateRandomMovement` ("For local
player generate movement commands") is registered through the same mechanism,
which is the switch the live probes flipped.

## Camera

One dense var block, v2b `0x7ff7c86957cc..0x7ff7c86acba4`, registered from a
stub at v2b `0x7ff7c52c243e..0x7ff7c52c447e` (exec `0x7ff753aa243e`):

* toggles/latency: `camera.EnableUpdate`, `camera.forceInput`;
* first-person animation blend: `camera.FirstPersonAnimationWeight`,
  `camera.FirstPersonAnimationRollWeight`, `camera.ShowFirstPersonAnimationOffset`
  (the FP view-model/anim coupling) — defaults **1.0 / 1.0 / 0** (read
  2026-10-19: v2b `0x7ff7c8695734`, `…574c`, `0x7ff7c89caa50`);
* gamepad feel: `camera.gamePadCameraDampening`, `camera.useGamePadCameraDampening`,
  `camera.gamepadStickPower`, `camera.gamepadTriggerPower`;
* interpolation (all inverse speeds; **defaults read 2026-10-19**):
  `lookInterpolationSpeedInverse` **0.1** (v2b `0x7ff7c86aeec0`,
  registration call `0x7ff7c52c3e1a`),
  `positionInterpolationSpeedInverse` **0.0** (`0x7ff7c89ee278`,
  call `0x7ff7c52c3f0a`), `offsetInterpolationSpeedInverse` **0.1**
  (`0x7ff7c86aeec4`, call `0x7ff7c52c3ffa`),
  `fovInterpolationSpeedInverse` **0.0** (`0x7ff7c89ee27c`,
  call `0x7ff7c52c40ea`), `rollInterpolationSpeedInverse` **0.1**
  (`0x7ff7c86aeec8`, call `0x7ff7c52c41da`);
* rates: `rollSpeed` **1.0** (`0x7ff7c86aeecc`), `moveSpeed` **0.1**
  (`0x7ff7c86aeed0`), `lookSpeed`, `FOVSpeed` — the values are per-tick
  (0.1 m/tick at 60 Hz is 6 m/s); their units are not read;
* projection: `camera.FirstPersonNearZ`, `camera.FirstPersonFarZ`
  (**0.01 / 1638.4 m**, ref at v2b `0x7ff7c56b252e`);
* the base FOV is a **settings** variable, not a `camera.*` one:
  `HorizontalFOV` (name v2b `0x7ff7c87f2f9c`) default **103.0°**, written at
  v2b `0x7ff7c51dd6a8` to `0x7ff7c8f83db8`. The client fixes the horizontal
  FOV and derives the vertical one from the aspect ratio.

The per-frame code that consumes the inverse speeds and rates is **still
cold**: no read sites for the five interpolation defaults appear in the
patched program, so that update is not in the captured pages. The FP camera
setup is a separate readable path at v2b `0x7ff7c568c5b0` (current build); its
60 deg angle, FP clip planes and animation FOV blend are in `camera.md`. The
3P block (`3PPositionOffsetY.*`, `3PDistanceSmoothing.chaseRate`) is
likewise data-only.

A camera-shake var (`localhiteffects.3PcameraShake`, v2b `0x7ff7c869e64c`) and a
`CameraShake` name (v2b `0x7ff7c87f504c`) tie weapon/hit feedback into the
camera. This is the camera's whole tunable surface, and it is the join point
for the live player-node trace: the camera interpolates toward the mover's
output at these inverse speeds.

## Audio

The client statically links **Wwise 2022.1** (Audiokinetic): the module carries
the SDK's own source paths and plugin names, e.g.

* `D:\DEV\Prometheus\3rdPartyArchive\Audiokinetic\Wwise2022\SDK\source\SoundEngine\Plugins\Effects\AkCompressor\AkCompressorFX.cpp`
  (and `AkExpander`, `AkFlanger`, `AkGain`, `AkDelay`, `AkConvolutionReverb`);
* `C:\Jenkins\ws\wwise_v2022.1\Wwise\SDK\source\SoundEngine\Plugins\Common\...`;
* convolution-reverb runtime diagnostics ("Soundbanks have been generated with
  convolution reverb parameters that do not match sound engine runtime
  conditions…") — i.e. the soundbanks and their reverb configuration are
  shipped data.

139 strings match the audio sweep. The game-side hook found so far is a debug
surface, `"Draw SoundSystem Info"` (v2b `0x7ff7c86a24cc`, referenced at exec
`0x7ff753be6aee`), plus volume vars such as `LobbySFXVolume`
(v2b `0x7ff7c88ed014`).

Consequence for the rewrite: audio is middleware to *call*, not to
reimplement — the faithful path is Wwise event posting against the shipped
soundbanks, not a DSP reimplementation. The event-posting sites are the next
read (the `PostEvent`-shaped calls into the Wwise engine are reachable from the
same string block).

## Method and limits

Every address above was found by RIP-relative scan of the module image for a
string anchor, then read in both stacks where a function existed (Ghidra
project for `module_v2full.bin`; REA for the region PE that carries the
registration stub). The name-decode and registration call sequence agree
between the two. Sites whose bytes are absent from the capture, or that live in
the per-var stubs the analyses never reached, are recorded as addresses and
read by disassembly window rather than by whole-function decompilation.

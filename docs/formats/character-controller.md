# The character mover: the entity component behind movement

## Current state

- **Mover data and arithmetic: read/measured.** Slots, jump, and the
  accel/friction/gravity rates, including crouch, air control and reversal,
  are measured and verified against keyed traces.
- **Mover update path: mapped.** Advance -> invoker -> collision sweep. The
  concrete class is a registered engine class whose own name is "Unknown".
- **Control law: tier 3.** The client's ramp expression is obfuscated: no rate
  literals, no computed velocity store, encrypted vtable slots. `ow-sim` uses
  the measured replacement.
- **Open:** crouch timing on a keyed trace, the steep-slide rule, and the
  operator's feel check.

**Verify:**

```text
cargo run -p ow-sim --bin trace_diff -- keys D:/ow-dumps/trace/player_fast3_keys.jsonl --by-state
cargo test --release -p ow-sim
```

Addresses are v2b coordinates for build `2.25.0.0.154088` unless marked
otherwise. `exec = v2b - 0x71820000`, `range = v2b - 0x17AEC0000`; capture
bases and translations are in `docs/ghidra-project.md`. File-side values come
from the per-hero mover dump and do not move with the build.

## The component

`FUN_7ff64b7ea450` is the component type router. It binary-searches a 32-bit
type hash and writes a small component id. The pairing was extracted from the
compare leaves (`dev/component_table.txt`): **120 component ids**, each bound
to one hash. The anchors:

| id | type hash | class |
|---|---|---|
| 0x26 | `0x6EFDBD73` | `STUCharacterMoverComponent` |
| 0x38 | `0x15CB6532` | `STUWeaponComponent` |
| 0x3A | `0xA658CA1C` | `STUHealthComponent` |

Fifteen leaves resist the mechanical pairing and are recorded as gaps, not
forced.

The captured client holds a reflective field table for the mover component at
`0x7ff64dc80fb0` (51 scalar records, stride 0x38). Every scalar's runtime slot
is its STU offset **+ 4**; the named anchors match the allocation registrar
exactly. The volley object follows the same pattern (`dev/volley_slots.txt`).

## Per-hero data

The STU class file
`E:\CASCReader\OWLib\TankLib\STU\Types\STUCharacterMoverComponent.cs`
declares `[STU(0x6EFDBD73, 304)]` with scalar fields from STU offset 8. The
runtime component is therefore 0x130 bytes with 52 fields; the engine mover
object it configures is a different, 0x1c00-byte object.

The dump (`dev/mover_values.txt`) reads one instance per hero: 122 heroes, 10
not readable, 3 without a mover. Values, without invented names:

* `031fe7ee` = **5.5 for every readable hero**. A constant base-speed
  denominator, not the per-hero forward slot.
* Constants: `aebad1b0` 45, `b74245e8` 30, `6f25aa23` 6.2, `6b7fdb90` 10,
  `e0a0fe25` 15, `e54438f7` 1, `4e03e892` 2, `e631ba84` 3.5 for most heroes
  (5 and 7 for bot outliers `900`/`906` and `915`/`916`).
* Never overridden, equal to class defaults: `d51c467d`, `5d90dbdb`,
  `d9ecb731`, `5632f333`, `808374ca`, `f227cbb6`, `e91d1664`, `d4a3c694`,
  `fd92c072`, `1ff06061` (50, 8, 15, 0.5, 0.5, 1, 4.5, 1, 1, 1).
* `1972c687` varies widely (1.7, 2, 2.1, 2.3, 2.5, 2.825, 2.9, 4); the
  plausible jump-velocity slot, but it stays hash-labelled.
* `08ffaa0a`, `4a576bc2`, `7a70e3e3`, `915ff368` are small fractions
  (0.3/0.5/0.7/1/1.5), co-varying in filters consistent with input
  deadzone/ramp. They stay hash-labelled.

The published gameplay figures match the extracted shape and, where they
cover our heroes, the values:

| hash | role | install value | published figure | verdict |
|---|---|---|---|---|
| `19452F15` | forward base speed | 5.5; 6.0 for exactly three heroes | 5.5; 6.0 for Tracer/Genji/Anran | named: shapes match to the hero count |
| `CFD3E635` | sideways base speed | equal to `19452F15` | sideways = forward | named by construction |
| `61D6B4E9` | backpedal base speed | 4.95; 5.4 for the fast trio | 90% of forward | named and verified per pair |
| `0DDD18C3` | crouch speed | 3.0; 2/2.5 for a few | 3.0, a few exceptions | named with caveats |
| `B74245E8` | terminal fall speed | 30 | 30 m/s | two-source constant |
| `1972C687` | unlabelled | 1.7-4.0 | not covered | hash-labelled |

The published jump impulse 5.72 has no single matching float in the mover
dump. The measured trace gives **5.33 m/s**, and that measured value is what
`ow-sim` applies. Everything above is exposed by the
`ow-tank` mover reader; `MoverSpec::back_speed()` / `crouch_speed()` look the
hashes up. The other 40+ scalars stay unnamed rather than guessed into roles.

## The jump struct

The second inline struct, `m_9947871A` (`STU_97288E54`) at STU offset 72,
carries the vertical/jump configuration. The client's default comparator
hardcodes six fields, all bit-exact against the class file:

| hash | runtime slot | class default | client constant |
|---|---|---|---|
| `16882D22` | 0x50 | 20f | `0x41a00000` |
| `746B3248` | 0x54 | 20f | `0x41a00000` |
| `F6B890EC` | 0x58 | **0.98f** | `0x3f7ae148` |
| `D68B7D62` | 0x5c | 0.7f | `0x3f333333` |
| `9FCDBE78` | 0x60 | 0.7f | `0x3f333333` |
| `766AFA5A` | 0x64 | 0.2f | `0x3e4ccccd` |

`0.98 m` is the published jump apex, and the mover's own defaults carry it.
The **live** pair is not the class default: the traced arc is 5.33 m/s at
17.37 m/s^2 (0.733 m discrete apex). A gravity scale of `0.8685 x 20` would
produce 17.37; unverified and recorded as a candidate.

## Descriptor and field table

The mover descriptor sits at `0x7ff64dc80980` (hash slot `+0xb8` equals
`0x6EFDBD73`): size **0x130**, **52 fields**, methods at `0x7ff64afc2160`,
`0x7ff64afc22e0`, `0x7ff64afc2370`, `0x7ff64afc24b0`, parent
`0x7ff64ddcffd0`. The health descriptor is at `0x7ff64dddb2d0` (0x68 bytes,
12 fields).

Field entry layout (stride 0x38, triple-verified):

```text
+0x00 u64 kind        (node vtable 0x7ff64ce43a00)
+0x08 u64 offset      (STU offset; runtime slot = offset + 4 for scalars)
+0x10 u64 field descriptor object
+0x30 u64 field hash
```

The walker reads offset at +8 and descriptor at +0x10; the health array
framing has exactly one alignment; the mover hash column reproduces
`m_1302B05C`, `m_9947871A`, `m_761CC74E` in order.

| # | offset | hash | # | offset | hash |
|---|---|---|---|---|---|
| 0 | 0x8 | `1302b05c` | 26 | 0xd4 | `4e03e892` |
| 1 | 0x48 | `9947871a` | 27 | 0xd8 | `142da8d4` |
| 2 | 0x68 | `761cc74e` | 28 | 0xdc | `031fe7ee` |
| 3 | 0x78 | `1972c687` | 29 | 0xe0 | `8895a9c8` |
| 4 | 0x7c | `7a70e3e3` | 30 | 0xe4 | `ec289987` |
| 5 | 0x80 | `4a576bc2` | 31 | 0xe8 | `c2c40969` |
| 6 | 0x84 | `03e04168` | 32 | 0xec | `b74245e8` |
| 7 | 0x88 | `915ff368` | 33 | 0xf0 | `975f21a5` |
| 8 | 0x8c | `d97b3735` | 34 | 0xf4 | `b99ec983` |
| 9 | 0x90 | `d4ae9d05` | 35 | 0xf8 | `cef4898c` |
| 10 | 0x94 | `5c6b4602` | 36 | 0xfc | `e91d1664` |
| 11 | 0x98 | `6b7fdb90` | 37 | 0x100 | `f227cbb6` |
| 12 | 0x9c | `e54438f7` | 38 | 0x104 | `808374ca` |
| 13 | 0xa0 | `aebad1b0` | 39 | 0x108 | `06b1ef76` |
| 14 | 0xa4 | `08ffaa0a` | 40 | 0x10c | `3d76c05d` |
| 15 | 0xa8 | `cfd3e635` | 41 | 0x110 | `b31ad888` |
| 16 | 0xac | `61d6b4e9` | 42 | 0x114 | `4a5244c1` |
| 17 | 0xb0 | `19452f15` | 43 | 0x118 | `5d90dbdb` |
| 18 | 0xb4 | `b470b8da` | 44 | 0x11c | `d9ecb731` |
| 19 | 0xb8 | `aa6f4ce1` | 45 | 0x120 | `5632f333` |
| 20 | 0xbc | `0ddd18c3` | 46 | 0x124 | `d51c467d` |
| 21 | 0xc0 | `daa0618b` | 47 | 0x128 | `6db41d17` |
| 22 | 0xc4 | `e631ba84` | 48 | 0x12a | `d4a3c694` |
| 23 | 0xc8 | `d24bf4e6` | 49 | 0x12b | `fd92c072` |
| 24 | 0xcc | `e0a0fe25` | 50 | 0x12c | `1ff06061` |
| 25 | 0xd0 | `6f25aa23` | 51 | 0x12d | `5aa2e07c` |

## Why the update is not on the class surface

The runtime component is constructed from the STU class (allocator
`Alloc(0x130,0)`), and its constructor installs three interface vtables
(`+0`, `+8`, `+0x48`). The four mover statics that were gaps are all
introspection: allocator, constructor, default comparator and default
provider. A reference sweep over the exec capture finds **5** referencing
sites for the mover descriptor and **1** for health, every one a lazy
registration stub, introspection attach, or a `lea rax,[rip+...]; ret` getter. No gameplay code names the class, descriptor
or vtables.

The consumer is therefore reached through the **physics body the component
configures**, not through the component's own surface. The engine object is:

* 0x1c00 bytes, registered by `FUN_7ff7c5540870` as class `"Unknown"`;
* factory table `0x7ff7c7d8dfd0`, concrete constructor `FUN_7ff7c5540750`;
* concrete vtables `0x7ff7c7d8e000` (+0) and `0x7ff7c7d8e0b8` (+0x20);
* interface subobject at `+0x1c0`; the component attach cascade
  `FUN_7ff7c5c20bd0` installs the mover interfaces (`FUN_7ff7c59ff390`).

The router's pairing is the cross-check: id `0x26` is
`STUCharacterMoverComponent`, size 0x130, parent `STUMirroredEntityComponent`.
The schema name is known even though the engine class registers as "Unknown".

## Update path

The character object is 0x1c00 bytes. Fields read by the movement code:

| offset | role |
|---|---|
| `+0x10d0` | u32 flags word (`0x80000c`, `0x20000`, `0x80000`) |
| `+0x10ec` | float timer, compared against input `[7]` |
| `+0x1140..0x114c` | 4-float **local wish vector** |
| `+0x1520`/`+0x1524` | u32 state-id pair, compared against input `[0x114]`/`[0x115]` |
| `+0x1a77..0x1a79` | movement state bytes (0x7b/0x7c) |
| `+0x1a90..0x1acc` | 4x4 world transform (translation at `+0x1ac0`) |
| `+0x1ad8` | physics world handle |
| `+0x1b80..0x1bbf` | committed transform: position quad + three basis vectors |
| `+0x1cd0` | pointer compared against the null sentinel |

The chain:

* **Advance** `FUN_7ff7c59ff690` (exec `FUN_7ff7541df690`; interface vtable
  slot 15) computes the desired position from the object's velocity-like quad
  at `+0xc0..0xcc`: `out_i = (a * X_i + pos_i) * b + off_i`. Its character
  path is the collision sweep; its plain-body path is that formula.
* **Step query** `FUN_7ff75625e790` (exec) is called through
  `FUN_7ff756225fe0(worldHandle, stepState, request)`. The caller sets
  constants 0x10 and 0x20, passes a 512-byte scratch buffer, and sizes/fills
  **two arrays of 0x20-byte elements** (pointer/capacity at
  `+0x200`/`+0x208` and `+0x610`/`+0x618`). The "character step" is a query
  with two output lists, not a bare integrator.
* **Invoker** `FUN_7ff7c5541260` (the 0x1c00 class's slot-15 override) directly
  calls the advance, calls its own slot 21, stores the new position into
  `+0x1b80`, then dot-products the movement delta against every contact normal
  in the step state's output list (`[state+0x200]`, 0x20-byte elements, normal
  at `+0..+8`). A maximum above `_DAT_7ff7c86a72f0` dispatches a per-id
  collision handler. The invoker is advance + collision notification, not the
  control.
* **Collision sweep** `FUN_7ff7c5b14a84` builds a session
  (`FUN_7ff7c7a45e40`), then sweeps `(i/3) * delta` for `i = 1..3`
  (`DAT_7ff7c7e1ae88` = 0.3333333) through `FUN_7ff7c7a46260` +
  `FUN_7ff7c7a45fe0`, with a request carrying 1/60 and 0.005, and the
  constant 5/3 stored before session create.
* **Per-frame state machine** `FUN_7ff7c5a12c50` reads flags, timer and state
  id; writes the state byte `+0x1a79`; transforms the local vector at `+0x1140`
  by the matrix at `+0x1a90`; calls the ground probe `FUN_7ff7c5a0e610`
  conditionally; then runs `FUN_7ff7c5a14c60`, `FUN_7ff7c5a18450`,
  `FUN_7ff7c5a186a0`.
* **Wish-vector update** `FUN_7ff7c59fd050` reads `+0x1140..0x114c`, calls
  `FUN_7ff7c5a0ce70` with the state-id pair, writes the result back, then
  applies a difference of two vectors.
* **Frame conversion** `FUN_7ff7c5a0ce70` takes a matrix from
  `FUN_7ff7c5a10210`, inverts it (adjugate + `rcpps` + one Newton refinement),
  and transforms the vector by the inverse. It converts coordinate spaces; it
  is **not** the input-to-velocity ramp.
* **Data applier** (in the `0x7ff7c59e8xxx` band) walks the component's field
  records (`[component+0x1a8 + 8i]`) and stores converted floats into
  consecutive runtime slots `+0xac + 4i`: the crouch speed at `+0xc0`, the
  speed family at `+0xe0`/`+0xe4`, `daa0618b` = 35, `e631ba84` = 3.5,
  `e0a0fe25` = 15 for most heroes.
* **Visual smoothing** `FUN_7ff7c59f7a70` lerps the display transform:
  `out = (dst - src) * k + src`, repeated per 16-byte lane at
  `+0x390`/`+0x3a0`. This explains the trace differ's "settling" class: after
  a stop, the last millimetres are this lerp, not mover integration. The
  traced node is the display/animation-side transform.

## Measured movement

The mover integrates **once per frame** with the frame's own dt, not at the
world's fixed 1/30 s tick. The keyed traces write once per frame; 189 intervals
give 6.46 samples at 200 Hz = **0.0323 s**, and the plateau step repeats to all
bits (0.1777004 m) with only ~0.6% slip on individual writes.

Measured at dt = 0.032309 s:

| quantity | measurement | value |
|---|---|---|
| forward/side speed | plateau step 0.1777004 m | **5.500 m/s** = `19452F15` |
| backpedal | plateau step 0.1599036 m | **4.950 m/s** = `61D6B4E9` = 0.9x forward |
| diagonal (W+D) | same plateau | full speed, input normalised |
| acceleration | 3.9216 m/s gained in first full frame from rest | **121.4 m/s^2** |
| friction | constant 2.345 m/s shed per frame | **72.6 m/s^2** |
| gravity | 0.5613 m/s shed per frame; variance tracks dt | **17.37 m/s^2** |
| jump impulse | first-frame dy 0.15426 m; discrete ascent 0.7333 m | **5.33 m/s** |
| airborne, no input | step holds its exact bits through the fall | **preserved** |
| airborne with direction | step grows 0.00533 m/frame until the slot | **18.5 m/s^2** = `AIR_ACCEL` |
| crouch walk | plateau 0.04956 m at 1/60 s; clean segments 2.960-3.011 m/s | **3.000 m/s** = `0DDD18C3` |
| crouch transition | 5.5 -> 3.46 -> 2.97 and 2.97 -> 4.16 -> 5.54 | rate-limited, 1-2 frames, no clamp or reset |
| reverse | 5.5 -> -0.5..-4.0 -> plateau | **210-231 m/s^2**, above ACCEL+FRICTION (194) |
| direction | ramp parallel to `(sin yaw, cos yaw)` | vector ramp, not per-axis |

The approach is along the velocity **vector**: a per-component clamp would
swing the direction during a turn, and the traced ramp keeps it. Release sheds
a constant speed, not a proportion of `v` (2.35 at v = 5.1 and 2.33 at
v = 2.7).

## Slope following

On walkable ground the mover keeps the **slot speed along the ground surface**.
The keyed backpedal climb (`player_keyed.jsonl`, i=352, frames 22-36) walks at
a constant horizontal 4.699 m/s and vertical +1.523 m/s (17.9 degrees);
`sqrt(4.699^2 + 1.523^2) = 4.940 m/s`, the backpedal slot 4.95 to 0.2%. The
horizontal component is `speed * cos(slope)` and the vertical
`speed * sin(slope)`, both constant while the slope is constant. The heading
is the wish's, not a rotated one: 9.7 degrees through the climb and -170.5
degrees on the forward run down the same ramp, the exact reverse to 0.2%.

The steeper case stays unmodelled. A strafe run (`player_keyed.jsonl`,
i=616..760) descends 2.51 m over 10.12 m and has a straight constant phase
(43 writes) at 4.2539 m/s horizontal and -1.9738 m/s vertical (24.89 degrees),
magnitude **4.6895 m/s**, which is 0.853x the 5.5 walk slot. Its grade equals
its instantaneous grade and its direction is constant, a fall-line-shaped
descent; it ends where the traced jump happens. The trace cannot say whether
the client called it grounded: the samples carry no body flags, `vel` is zero,
and with no yaw field the wish direction during the slide is unrecorded. This
is a slide/lost-footing state, not ground-following; its rule belongs to the
obfuscated control (tier 3) or to terrain behaviour.

`Controller::ground_normal` (default `[0,1,0]`) scales the wish by the
directional slope's cosine and carries the sine share as `slope_vertical` while
grounded. The golden test
`slope_following_holds_the_slot_speed_along_the_surface` drives the traced
17.9-degree grade and asserts |v| = 4.95 to 0.002 plus the traced components.
The harness sandbox is flat, so the path is dormant there and flat traces are
unchanged.

## Jump, gravity and verification

The differ verifies the vertical model against the trace, not only unit tests:

```text
no divergence across 18 compared write(s): ... max vertical delta error 0.0014184 m
```

on `player_keyed.jsonl` (standing jump at i=852). The flat regression
(`player_fast3_keys.jsonl`) remains **101 compared writes, max step deviation
0.0018616 m** (1.0% of a 0.178 m step), with the client's frame-jitter band and
environment-affected wall-slide/terrain writes screened out.

The screens and corrections that make those numbers meaningful:

* **Yaw inference.** `player_keyed` carries no yaw; the yaw is recovered from
  the first free-movement step as `yaw = atan2(dx, dz)`, the client's own
  convention. It is measured on flat straight plateaus to **0.006 degrees**
  (w: -170.4387 against 189.5668; d: -90.0018; a: +89.9979; s: 180 +/- 0.29).
  A first step can read up to ~0.5 degrees off facing when previous velocity
  blends into it.
* **Per-segment vertical baseline.** A trace crosses several ground heights
  (0.711, -1.673 at the jump, -2.289 at the end), so each compared segment is
  aligned to the client's height at its head and keeps its own ground through a
  post-release descent.
* **Terrain screens.** A step is a vertical move over 1 cm (the display lerp
  stays under 6 mm). A sustained one-signed vertical over 75% of a segment is a
  ramp, ledge or slide; 75% rather than 60% because a jump's key release
  splits its arc. A net height change alone is not terrain.
* **Write gaps are crossed, not skipped.** A standing character writes
  nothing, so the replay advances missing frames with the held input; friction
  decays the velocity exactly as the client's did. Without this, a jump from
  rest after a long idle left the ground at 0.81 m/s.
* **The jump is a rising edge.** The movement ramp has a one-write delay, but
  the traced jump's first space write already carries the rise, so the impulse
  uses the current write's keys and fires on `space` and not the previous
  write's `space`. No trace distinguishes held from edge (every traced jump
  releases before landing); the edge reading is the sim's contract, not a
  measurement of the client's edge handling.
* **The arc is compared by deltas.** The take-off is 1-2 frames earlier than
  the first write that shows it, so absolute dy is one write out of phase. The
  deltas are phase-invariant and exactly `g * dt^2`; they match to **1.4 mm**
  across the whole arc against a 6 mm band. Landings are snap clamps and are
  skipped.

Crouch, air control and reversal were measured from dedicated captures
(`player_ctrl*.jsonl`):

| capture | compared | client-jitter band | env-screened | first divergence |
|---|---|---|---|---|
| `player_ctrl4.jsonl` | 3696 | 3600 | 2124 | 0.0080 m (4.5% of step) |
| `player_ctrl3.jsonl` | 3137 | 3000 | 3403 | 0.0066 m (3.7%) |
| `player_ctrl2.jsonl` | 2641 | 2413 | 3894 | 0.0420 m (wall cycles) |

`player_ctrl4` reports **max vertical delta error 0.0019 m** over 300 s,
jumps included. Horizontal residuals sit at 2-5% of a step, concentrated at
reversal writes and writes where the client's own frame time varied. That is
the sampling limit (200 Hz resolving a 30 fps client's writes to +/-5 ms,
+/-15% on one interval), not a model error. The stall screen removes segments
walking into geometry (a wall slide keeps every step small, so a
size-change test cannot see it); it screens 2124 writes in `player_ctrl4`.

The 60 fps practice-range capture gates because the client is **sampled**, not
resolved: the steps are exact and the per-segment mean dt recovers the effective
frame time.

## Evidence

* Component router pairing: capstone replay over `FUN_7ff64b7ea450`
  (`dev/component_table.txt`).
* Registrar slot pairing: capstone walk over `FUN_161b423db83`+; the mover
  pairing is spot-checked inline (`dev/volley_slots.txt` for the volley).
* STU class file: the operator's own `OWLib` checkout, read as a path
  reference, not vendored.
* Mover data: the per-hero dump. Every field lands on an expected parametric
  pattern (constant rows, co-varying rows, bot outliers), which a wrong offset
  would not produce.
* Movement rates: keyed traces `player_fast3_keys.jsonl`, `player_keyed.jsonl`
  and `player_ctrl*.jsonl`, replayed by `trace_diff keys --by-state`.
* Descriptor layout: the walker reads offset at +8 and descriptor at +0x10;
  the health array framing has one alignment; the mover hash column
  reproduces three known names in order.

## Verdict: the control is obfuscated (tier 3)

Every readable route to the ramp has been exhausted. The control is
**obfuscated**, most plausibly a bytecode/VM layer using the same
`(ptr + K) ^ K2` pointer idiom seen in the Domino handle table and the mover
class vtables.

The evidence:

1. The concrete class's second vtable (`0x7ff7c7d8e0b8`) carries encrypted
   slots from 13 onward (`0xa24b69a6c1deedc9`, `0xa5ae2a583bd75a70`, ...)
   where slots 0-12 are ordinary code pointers.
2. `FUN_7ff7c5a191c0` is 34 bytes that decompile to hundreds of kilobytes of
   repeated `*in_RAX = *in_RAX + cVar8`; decompiler-hostile, not real code.
3. No literal for any measured rate exists in `module_v2full.bin`, the 1695
   exec region dumps, or the warm capture: 121.4, 72.6, 17.37, 5.33, 5.5,
   4.95 all zero hits. 60.0/36.0 exist but are never paired within 0x60 bytes,
   and 60.0 is also a frame-rate constant.
4. No computed store to the mover's velocity quad in any encoding (movss,
   movups, movaps, movdqu, mov32, mov64, imm32, SIB) in any capture. The only
   writes are the data applier's scalars and reset/zero loops.
5. No movement code reads the mover's speed fields. The `[obj+0xb4]` readers
   are the field comparator and the script opcode applier; the `[obj+0xe0]`
   readers are skeletal-animation functions. The rest were data false
   positives.

The **behaviour** is fully measured and verified (101/101 comparable writes
inside the frame-jitter floor; 0.0018616 m max step deviation; 18/18 jump
writes at 0.0014184 m vertical error), and the measured arithmetic is
implemented in `ow-sim::movement` with golden-vector tests. The client's own
expressions are unrecoverable here and are **declared tier 3** under
`AGENTS.md`: the numbers stand as trace-measured, not as a reading. Crouch
transition timing remains measurable from a trace rather than readable from
code.

## Open

- Crouch transition timing on a keyed trace.
- The steep-slide (24.89-degree) rule; currently unmodelled and distinct from
  ground-following.
- The operator's feel check in `ow-view --play`.
- The mover's real class name (registered as `"Unknown"`) and the startup
  walker for the registrar list.

# The character mover

The mover is the client's first-person character controller: a component that
configures a larger character object, and an advance/invoker pair that
integrates the per-frame step.

## Current state

- **Data: read.** The component schema, the per-hero slots and the jump struct
  are read from the install.
- **Update path: partly read.** The component registration, the character
  object, the advance, the invoker and the state machine are located in the
  current image.
- **Open:** the input→velocity ramp. The advance integrates a step; the
  function that computes that step from the wish vector and the current
  velocity is not located.
- **Implementation status:** no movement is implemented. The old
  trace-measured approximation was deleted with `ow-sim/movement.rs`; the
  client's own control law is the only acceptable source.

## The component

`RegisterMoverComponent` (`0x14017a680`) registers
`STUCharacterMoverComponent`: type hash `0x6EFDBD73`, component id `0x26`,
size `0x130`, 52 fields. The constructor (`0x14111b120`) installs the three
interface vtables at component `+0`, `+8` and `+0x48`.

At runtime every scalar's slot is its STU offset **+ 4**:

| STU offset | runtime slot | meaning |
|---|---|---|
| `0x08` | `0x0c` | first scalar (schema header) |
| `0x48` | `0x4c` | jump struct (`STU_97288E54`) |
| `0x68` | `0x6c` | second inline struct |
| `0xb0` | `0xb4` | forward speed |
| `0xb4` | `0xb8` | side speed |
| `0xb8` | `0xbc` | back speed |
| `0xbc` | `0xc0` | crouch speed |
| `0xec` | `0xf0` | terminal fall speed |

The per-hero values are read by `ow-tank`'s mover reader. The named anchors:

| hash | role | install value |
|---|---|---|
| `19452F15` | forward base speed | `5.5`; `6.0` for three heroes |
| `CFD3E635` | sideways base speed | equal to forward |
| `61D6B4E9` | backpedal base speed | `4.95`; `5.4` for the fast trio |
| `0DDD18C3` | crouch speed | `3.0`; `2.0`/`2.5` for a few |
| `B74245E8` | terminal fall speed | `30` |

The remaining 40+ scalars stay hash-labelled; none is guessed into a role.

## The jump struct

`STU_97288E54` at STU offset `72` carries the vertical configuration. The
client's default comparator hardcodes six fields, bit-exact against the class
file:

| hash | runtime slot | default |
|---|---|---|
| `16882D22` | `0x50` | `20f` |
| `746B3248` | `0x54` | `20f` |
| `F6B890EC` | `0x58` | `0.98f` |
| `D68B7D62` | `0x5c` | `0.7f` |
| `9FCDBE78` | `0x60` | `0.7f` |
| `766AFA5A` | `0x64` | `0.2f` |

## The character object

The component configures a 0x1c00-byte character object. `ConstructMoverBase`
(`0x140ca37d0`) builds the 0x1b70-byte base subobject and installs the base
vtable at `0x1431101e8`; `DestructMoverBody` (`0x1407ff270`) frees it.

Fields the movement code reads:

| offset | role |
|---|---|
| `+0x10d0` | u32 flags word |
| `+0x10ec` | float timer |
| `+0x1140..0x114c` | 4-float **local wish vector** |
| `+0x1520`/`+0x1524` | u32 state-id pair |
| `+0x1a77..0x1a79` | movement state bytes |
| `+0x1a90..0x1acc` | 4x4 world transform (translation at `+0x1ac0`) |
| `+0x1ad8` | physics world handle |
| `+0x1b80..0x1bbf` | committed transform: position quad + three basis vectors |
| `+0x1cd0` | null-sentinel pointer |

## The update path

| function | address | role |
|---|---|---|
| `MoverAdvance` | `0x140ca7f30` | the per-frame position integrator (base vtable slot 15) |
| `MoverInvoker` | `0x1407ffc50` | calls the advance, stores the new position at `+0x1b80`, dispatches contact normals |
| `MoverStateMachine` | `0x140cbacc0` | reads flags/timer/state id, writes the state byte `+0x1a79`, transforms the local wish vector by the transform |
| `MoverWishUpdate` | `0x140ca5940` | reads `+0x1140..0x114c`, resolves the state-id pair, writes the result back |

`MoverAdvance` has two paths:

- **plain body:** `out_i = (a * X_i + pos_i) * b + off_i`, with `X` the
  **static local offset** at `+0xc0..0xcc`;
- **character:** the collision sweep, using the walk-state bytes at
  `+0x1a79`/`+0x1a7c`, the step state and the body/world handle at `+0x35b`.

`MoverInvoker` is advance plus collision notification, not the control law.
The per-frame step's source is the wish vector at `+0x1140` and the transform
at `+0x1a90`, with the advance's `param_5`/`param_6` scales.

## What the current image shows (2026-10-25)

`MoverAdvance` is not a leaf. Its character path computes a desired position
and then runs the collision sweep:

```text
desired_i = (param_5 * body[0x18_i] + wish_i) * param_6 + param_4_i
RunCharacterControllerSweep(body[0x35b], step_state, desired)
new_pos   = FUN_142cbcd10(body[0x35b], old_pos)
```

`param_3` is the local wish vector; `param_4` is the current position;
`param_5` and `param_6` are the per-frame scales. `MoverInvoker` passes eight
arguments through to it:

```text
MoverInvoker(mover, out_pos, wish, pos, scale_a, scale_b, step_state, world)
```

`MoverInvoker` is reached through the interface vtable `0x142fdbf40` slot 9.
The function that computes `wish`, `scale_a`, `scale_b` and the step state for
a given input frame is **not yet located**. That is the ramp/control law.

Two more read pieces are on the same path:

- `ApplyStateModifiers` (`0x140cbcbe0`) evaluates the mover body's 0x38-byte
  state-modifier records at `+0x870..` and writes the movement parameter block
  at `+0x5d0..+0x67c` (base/effective pairs) plus the flags at
  `+0x652`/`+0x659`/`+0x1a81`.
- `SubmitMoverPhysicsCommand` (`0x140cc03d0`) calls
  `BuildMoverPhysicsCommand` (`0x140cb74a0`) and dispatches the command to
  Domino through `SubmitCharacterControllerCommand` (`0x142cf5770`).

So the control law is one layer above `MoverInvoker`, and the movement
parameters it consumes are the body block `+0x5d0..+0x67c`. The next read is
the caller of `MoverInvoker` and the producer of that block.

## The open ramp

The field index finds **no per-frame writer** for the body's `+0xc0..0xcc`;
the stores are constructors and bulk copies. It is therefore a static local
offset, not a stored velocity. The state modifier is an inline vector: the
pointer at body `+0x870` points at `+0x880`, the count is at `+0x878`, and the
capacity at `+0x87c`. Its records are populated at runtime.

The candidates, in order:

1. the caller of `MoverInvoker` (vtable `0x142fdbf40` slot 9), which produces
   the wish/position/scales;
2. the state-modifier table (`[body+0x10e]`, 0x38-byte records) and the CASC
   state data it copies from;
3. the movement statescript graph, read with `ow-tank`'s graph tools;
4. a force path into `dmBody.force` (`+0x130`) that lets the island solve
   produce the velocity.

Whichever is read, the implementation must follow it operation for operation,
not match a trace.

## Collision

The character path is a collision sweep, not a bare integrator. The client's
step query, ledge and slope handling are not read. The client has no gameplay
collision implementation yet; the map mesh is visual only. The map's collision
chunks are named in `formats/maps.md`.

## Verification

Once the ramp is read:

```text
cargo test --release -p ow-sim
python dev/verify_harness/cli.py all
```

A golden vector must reproduce the client's per-step bits; a timeline must
report no divergence. Until then the movement code is a deviation, not a
result.

# Domino: the physics library

Domino is the client's rigid-body physics library: a world, bodies, fixtures,
broadphase trees and a fixed-step island solver. How the local character uses
it — its own mover step, a `dmBody`, or both — is part of the movement
question in `formats/character-controller.md`.

## Current state

- **Located in the current image:** the world constructor, the tick driver,
  the substep driver, the inv/dt writer, the broadphase re-init, the constraint
  dispatch, the rotation-inertia solve and the residual query.
- **Open:** the runtime schema (class/field offsets), the exact integration
  arithmetic and the configured tick pair. The earlier notes for these came
  from a runtime observation of another build; they are not a current-image
  finding and are not reproduced here.

## The current-image functions

| function | address | role |
|---|---|---|
| `DmTickKeeper` | `0x14208c070` | the frame-side tick keeper: decides how many steps run |
| `DmWorldConstruct` | `0x142cbeb70` | the world constructor |
| `DmWorldInvDt0Writer` | `0x142cc3050` | writes `m_inv_dt0` from the step's dt |
| `DmWorldSubstepDriver` | `0x142cc3630` | the substep driver |
| `DmBroadphaseReinit` | `0x142cbdc60` | re-initialises the broadphase |
| `ConstraintDispatch` | `0x142cd9c90` | dispatches a constraint to its solver |
| `RotationInertiaSolve` | `0x142d07170` | the rotational inertia solve |
| `DmResidualQuery` | `0x142cdf710` | the residual query that ends a catch-up loop |

The class descriptors and the body/world layout are in the image's data; they
must be read from the constructors and the field arrays before any offset is
used.

## What must be read next

1. **`dmWorld` and `dmBody` layout.** Read `DmWorldConstruct` and the body
   constructor; record every field offset with its type.
2. **The tick.** Read `DmTickKeeper` and `DmWorldSubstepDriver`: the dt value,
   the number of steps, the catch-up rule and the interpolation phase.
3. **The integration order.** Read the island solve from `ConstraintDispatch`
   and `RotationInertiaSolve`. The order of gravity, force, damping, velocity
   and position updates is the part that has to be exact.
4. **The local character link.** Confirm whether the mover writes a
   `dmBody.force`, uses a collision sweep, or both.

## Verification

Once the schema and the arithmetic are read, a pure body step gets a golden
vector and a multi-step run gets a first-divergence test. A formula that is
"close" is not a result.
